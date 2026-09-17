//! kotlinx.serialization.protobuf support.
//!
//! Implements enough of the protobuf wire format plus the
//! `ProtoBuf` / decoder shim surface to run plugin-generated serializers
//! from keiyoushi extension APKs (e.g. M+), following the same approach as
//! the JSON pipeline in `serialization.rs`: the host parses the wire bytes,
//! then drives the APK's own generated serializer bytecode through a shim
//! decoder (`Lkotlinx/serialization/protobuf/ProtoDecoder;`).
//!
//! Field numbers are recovered from `@ProtoNumber` annotation objects the
//! guest pushed onto its serial descriptors via `pushAnnotation`.

use super::*;
use crate::vm::object::WireValue;

pub(crate) type PR = Result<JValue, NatErr>;

// ---------------------------------------------------------------------------
// wire format parsing
// ---------------------------------------------------------------------------

/// Parses a protobuf message into `(field number, value)` pairs in wire order.
pub(crate) fn parse_wire(bytes: &[u8]) -> Result<Vec<(u32, WireValue)>, String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let (key, n) = read_varint(bytes, i).ok_or_else(|| "truncated tag".to_string())?;
        i += n;
        let field = (key >> 3) as u32;
        let wtype = (key & 7) as u8;
        if field == 0 {
            return Err("invalid field number 0".into());
        }
        let value = match wtype {
            0 => {
                let (v, n) = read_varint(bytes, i).ok_or_else(|| "truncated varint".to_string())?;
                i += n;
                WireValue::Varint(v as i64)
            }
            1 => {
                if i + 8 > bytes.len() {
                    return Err("truncated fixed64".into());
                }
                let mut b = [0u8; 8];
                b.copy_from_slice(&bytes[i..i + 8]);
                i += 8;
                WireValue::Fixed64(u64::from_le_bytes(b))
            }
            2 => {
                let (len, n) =
                    read_varint(bytes, i).ok_or_else(|| "truncated length".to_string())?;
                i += n;
                let len = len as usize;
                if i + len > bytes.len() {
                    return Err("truncated length-delimited data".into());
                }
                let v = bytes[i..i + len].to_vec();
                i += len;
                WireValue::Bytes(v)
            }
            5 => {
                if i + 4 > bytes.len() {
                    return Err("truncated fixed32".into());
                }
                let mut b = [0u8; 4];
                b.copy_from_slice(&bytes[i..i + 4]);
                i += 4;
                WireValue::Fixed32(u32::from_le_bytes(b))
            }
            other => return Err(format!("unsupported wire type {other}")),
        };
        out.push((field, value));
    }
    Ok(out)
}

/// Reads one base-128 varint; returns (value, bytes consumed).
fn read_varint(bytes: &[u8], start: usize) -> Option<(u64, usize)> {
    let mut result = 0u64;
    let mut shift = 0u32;
    let mut i = start;
    loop {
        let b = *bytes.get(i)?;
        result |= ((b & 0x7f) as u64) << shift;
        i += 1;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 70 {
            return None;
        }
    }
    Some((result, i - start))
}

// ---------------------------------------------------------------------------
// descriptor helpers (field number <-> element index via @ProtoNumber)
// ---------------------------------------------------------------------------

/// Reads the proto field number of an `@ProtoNumber` annotation object.
/// The annotation is a guest class whose `<init>(I)` stores the number into
/// its only int field; scan the instance fields for it.
fn anno_field_number(vm: &Vm, anno: JValue) -> Option<i32> {
    let JValue::Obj(id) = anno else {
        return None;
    };
    let obj = vm.arena.get(id)?;
    for f in &obj.fields {
        if let JValue::Int(v) = f {
            return Some(*v);
        }
    }
    None
}

/// Maps a wire field number to a descriptor element index:
/// 1. exact match against an `@ProtoNumber` annotation on the element,
/// 2. otherwise the protobuf default numbering (`index + 1`) for elements
///    without any annotation,
/// 3. otherwise `None` (unknown field — skipped by the decoder).
fn field_index_for(vm: &Vm, descriptor: JValue, field_no: u32) -> Option<i32> {
    let Some(Native::SerialDescriptor {
        elements,
        element_annotations,
        ..
    }) = payload(vm, descriptor)
    else {
        if std::env::var("DEXVM_TRACE").is_ok() {
            eprintln!("PROTO map f{field_no}: no SerialDescriptor payload");
        }
        return Some((field_no as i32) - 1);
    };

    if std::env::var("DEXVM_TRACE").is_ok() {
        let name = match payload(vm, descriptor) {
            Some(Native::SerialDescriptor { name, .. }) => name.clone(),
            _ => String::new(),
        };
        eprintln!(
            "PROTO map f{field_no}: descriptor name={name:?} elements={} annotations={}",
            elements.len(),
            element_annotations.len()
        );
        for (i, annos) in element_annotations.iter().enumerate() {
            let nums: Vec<String> = annos
                .iter()
                .map(|&a| format!("{:?}", anno_field_number(vm, a)))
                .collect();
            eprintln!("PROTO   elem{i} annos=[{}]", nums.join(","));
        }
    }

    for (i, annos) in element_annotations.iter().enumerate() {
        for &anno in annos {
            if let Some(num) = anno_field_number(vm, anno) {
                if num >= 0 && num as u32 == field_no {
                    return Some(i as i32);
                }
            }
        }
    }

    // Default numbering applies to un-annotated elements only.
    let idx = (field_no as i32) - 1;
    if idx >= 0
        && (idx as usize) < elements.len()
        && element_annotations
            .get(idx as usize)
            .is_some_and(|a| a.is_empty())
    {
        return Some(idx);
    }
    None
}

// ---------------------------------------------------------------------------
// decoder payload plumbing
// ---------------------------------------------------------------------------

/// Immutable view of the decoder's wire state.
fn dec_state(vm: &Vm, decoder: JValue) -> Option<(Vec<(u32, WireValue)>, usize)> {
    match payload(vm, decoder) {
        Some(Native::ProtoDecoder { fields, cursor, .. }) => Some((fields.clone(), *cursor)),
        _ => None,
    }
}

/// Mutable cursor access.
fn dec_cursor(vm: &mut Vm, decoder: JValue) -> Option<&mut usize> {
    match payload_mut(vm, decoder) {
        Some(Native::ProtoDecoder { cursor, .. }) => Some(cursor),
        _ => None,
    }
}

/// Takes the pending (field number, value) pair — the one consumed by the
/// last `decodeElementIndex` call — for use by an element decode method.
fn take_current(vm: &mut Vm, decoder: JValue) -> Option<(u32, WireValue)> {
    match payload_mut(vm, decoder) {
        Some(Native::ProtoDecoder {
            cur_field, cur_val, ..
        }) => {
            let field = (*cur_field)?;
            let value = cur_val.take()?;
            Some((field, value))
        }
        _ => None,
    }
}

/// Peeks the current value without consuming it.
fn peek_current(vm: &Vm, decoder: JValue) -> Option<(u32, WireValue)> {
    match payload(vm, decoder) {
        Some(Native::ProtoDecoder {
            cur_field, cur_val, ..
        }) => {
            let field = (*cur_field)?;
            let value = cur_val.clone()?;
            Some((field, value))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// value conversions (wire value -> JVM value)
// ---------------------------------------------------------------------------

fn wire_to_int(v: &WireValue) -> i32 {
    match v {
        WireValue::Varint(i) => *i as i32,
        WireValue::Fixed32(u) => *u as i32,
        WireValue::Fixed64(b) => *b as i32,
        WireValue::Bytes(_) => 0,
    }
}

fn wire_to_long(v: &WireValue) -> i64 {
    match v {
        WireValue::Varint(i) => *i,
        WireValue::Fixed32(u) => *u as i64,
        WireValue::Fixed64(b) => *b as i64,
        WireValue::Bytes(_) => 0,
    }
}

fn wire_to_bool(v: &WireValue) -> bool {
    match v {
        WireValue::Varint(i) => *i != 0,
        _ => false,
    }
}

fn wire_to_float(v: &WireValue) -> f32 {
    match v {
        WireValue::Fixed32(u) => f32::from_bits(*u),
        WireValue::Varint(i) => *i as f32,
        _ => 0.0,
    }
}

fn wire_to_double(v: &WireValue) -> f64 {
    match v {
        WireValue::Fixed64(b) => f64::from_bits(*b),
        WireValue::Varint(i) => *i as f64,
        _ => 0.0,
    }
}

// ---------------------------------------------------------------------------
// CompositeDecoder natives (class Lkotlinx/serialization/protobuf/ProtoDecoder;)
// ---------------------------------------------------------------------------

/// `Decoder.beginStructure(descriptor)` — the same host decoder serves as
/// the composite decoder for this nesting level.
pub(crate) fn p_begin_structure(_vm: &mut Vm, args: &[JValue]) -> PR {
    Ok(args[0])
}

/// `CompositeDecoder.decodeSequentially()` — protobuf decodes by walking
/// wire tags, so generated serializers must use the element-index loop.
pub(crate) fn p_decode_sequentially(_vm: &mut Vm, _args: &[JValue]) -> PR {
    Ok(JValue::Int(0))
}

/// `CompositeDecoder.decodeElementIndex(descriptor)` — consumes the next
/// wire tag and returns the matching descriptor index (or -1 when done).
/// Unknown field numbers are skipped, mirroring kotlinx-protobuf's default
/// behaviour of ignoring unannotated/unknown fields.
pub(crate) fn p_decode_element_index(vm: &mut Vm, args: &[JValue]) -> PR {
    let descriptor = args[1];
    loop {
        let Some((fields, cursor)) = dec_state(vm, args[0]) else {
            return Err(npe(vm));
        };
        if cursor >= fields.len() {
            return Ok(JValue::Int(-1));
        }
        let (field_no, value) = fields[cursor].clone();
        match field_index_for(vm, descriptor, field_no) {
            Some(index) => {
                if let Some(cursor) = dec_cursor(vm, args[0]) {
                    *cursor += 1;
                }
                // Stash the consumed entry for the element decode call that
                // immediately follows.
                if let Some(Native::ProtoDecoder {
                    cur_field, cur_val, ..
                }) = payload_mut(vm, args[0])
                {
                    *cur_field = Some(field_no);
                    *cur_val = Some(value.clone());
                }
                if std::env::var("DEXVM_TRACE").is_ok() {
                    let desc = match &value {
                        WireValue::Varint(i) => format!("varint({i})"),
                        WireValue::Fixed32(u) => format!("fixed32({u})"),
                        WireValue::Fixed64(b) => format!("fixed64({b})"),
                        WireValue::Bytes(b) => {
                            format!("bytes[{}] {:?}", b.len(), String::from_utf8_lossy(b))
                        }
                    };
                    eprintln!("PROTO decodeElementIndex field={field_no} -> idx={index} {desc}");
                }
                return Ok(JValue::Int(index));
            }
            None => {
                // Unknown field: skip it and keep scanning.
                if std::env::var("DEXVM_TRACE").is_ok() {
                    eprintln!("PROTO decodeElementIndex SKIP unknown field {field_no}");
                }
                if let Some(cursor) = dec_cursor(vm, args[0]) {
                    *cursor += 1;
                }
            }
        }
    }
}

fn p_decode_int_element(vm: &mut Vm, args: &[JValue]) -> PR {
    let Some((f, v)) = take_current(vm, args[0]) else {
        return Err(npe(vm));
    };
    let r = wire_to_int(&v);
    if std::env::var("DEXVM_TRACE").is_ok() {
        eprintln!("PROTO int f{f} = {r}");
    }
    Ok(JValue::Int(r))
}

fn p_decode_long_element(vm: &mut Vm, args: &[JValue]) -> PR {
    let Some((_, v)) = take_current(vm, args[0]) else {
        return Err(npe(vm));
    };
    Ok(JValue::Long(wire_to_long(&v)))
}

fn p_decode_boolean_element(vm: &mut Vm, args: &[JValue]) -> PR {
    if std::env::var("DEXVM_TRACE").is_ok() {
        eprintln!("PROTO bool element");
    }
    let Some((_, v)) = take_current(vm, args[0]) else {
        return Err(npe(vm));
    };
    Ok(JValue::Int(i32::from(wire_to_bool(&v))))
}

fn p_decode_float_element(vm: &mut Vm, args: &[JValue]) -> PR {
    let Some((_, v)) = take_current(vm, args[0]) else {
        return Err(npe(vm));
    };
    let f = wire_to_float(&v);
    Ok(JValue::Float(f))
}

fn p_decode_double_element(vm: &mut Vm, args: &[JValue]) -> PR {
    let Some((_, v)) = take_current(vm, args[0]) else {
        return Err(npe(vm));
    };
    let d = wire_to_double(&v);
    Ok(JValue::Double(d))
}

fn p_decode_string_element(vm: &mut Vm, args: &[JValue]) -> PR {
    let Some((f, v)) = take_current(vm, args[0]) else {
        return Err(npe(vm));
    };
    match v {
        WireValue::Bytes(b) => {
            let s = String::from_utf8_lossy(&b).into_owned();
            if std::env::var("DEXVM_TRACE").is_ok() {
                eprintln!("PROTO string f{f} = {s:?}");
            }
            Ok(new_str(vm, &s))
        }
        WireValue::Varint(_) => Err(npe(vm)),
        _ => Ok(new_str(vm, "")),
    }
}

fn p_decode_bytes_element(vm: &mut Vm, args: &[JValue]) -> PR {
    let Some((_, v)) = take_current(vm, args[0]) else {
        return Err(npe(vm));
    };
    match v {
        WireValue::Bytes(b) => alloc_arr(vm, "B", b.len(), move || {
            ArrayData::Byte(b.into_iter().map(|x| x as i8).collect())
        }),
        _ => Err(npe(vm)),
    }
}

/// `CompositeDecoder.endStructure(descriptor)` — nothing to clean up; the
/// decoder state dies with the shim object.
pub(crate) fn p_end_structure(_vm: &mut Vm, _args: &[JValue]) -> PR {
    Ok(JValue::Null)
}

// ---------------------------------------------------------------------------
// nested values: messages, lists, primitives
// ---------------------------------------------------------------------------

/// Allocates a child decoder over an embedded message's bytes.
fn proto_child_decoder(vm: &mut Vm, bytes: Vec<u8>) -> PR {
    let fields = parse_wire(&bytes).map_err(|e| {
        let _ = e;
        npe(vm)
    })?;
    alloc(
        vm,
        "Lkotlinx/serialization/protobuf/ProtoDecoder;",
        Native::ProtoDecoder {
            fields,
            cursor: 0,
            cur_field: None,
            cur_val: None,
        },
    )
}

/// Public entry used by `ArrayListSerializer.deserialize` when the decoder
/// is a protobuf decoder: consumes the repeated field's consecutive entries.
pub(crate) fn proto_list_deserialize(vm: &mut Vm, decoder: JValue, child_serializer: JValue) -> PR {
    proto_list_deserialize_inner(vm, decoder, child_serializer)
}

/// Decodes a repeated field: consumes the pending value plus every
/// consecutive wire entry with the same field number, decoding each through
/// `child_serializer`, and returns a java.util.ArrayList.
fn proto_list_deserialize_inner(vm: &mut Vm, decoder: JValue, child_serializer: JValue) -> PR {
    let (field_no, first) = peek_current(vm, decoder).ok_or_else(|| npe(vm))?;

    let mut items: Vec<WireValue> = vec![first];
    let total = fields_len(vm, decoder);
    loop {
        let cursor = match dec_cursor(vm, decoder) {
            Some(c) => *c,
            None => break,
        };
        if cursor >= total {
            break;
        }
        match fields_at(vm, decoder, cursor) {
            Some((f, v)) if f == field_no => {
                items.push(v);
                if let Some(c) = dec_cursor(vm, decoder) {
                    *c += 1;
                }
            }
            _ => break,
        }
    }

    let mut out: Vec<JValue> = Vec::with_capacity(items.len());
    for v in &items {
        out.push(wire_value_as(vm, v, child_serializer)?);
    }
    alloc(vm, "Ljava/util/ArrayList;", Native::List(out))
}

fn fields_len(vm: &Vm, decoder: JValue) -> usize {
    match payload(vm, decoder) {
        Some(Native::ProtoDecoder { fields, .. }) => fields.len(),
        _ => 0,
    }
}

fn fields_at(vm: &Vm, decoder: JValue, index: usize) -> Option<(u32, WireValue)> {
    match payload(vm, decoder) {
        Some(Native::ProtoDecoder { fields, .. }) => fields.get(index).cloned(),
        _ => None,
    }
}

/// Converts one wire value into a JVM value using the given child serializer:
/// builtins are converted directly; message serializers recurse.
fn wire_value_as(vm: &mut Vm, v: &WireValue, serializer: JValue) -> PR {
    match payload(vm, serializer) {
        Some(Native::PrimitiveSerializer(kind)) => {
            let kind = *kind;
            return Ok(match kind {
                crate::vm::object::PrimitiveSerializerKind::String => match v {
                    WireValue::Bytes(b) => new_str(vm, &String::from_utf8_lossy(b)),
                    _ => new_str(vm, ""),
                },
                crate::vm::object::PrimitiveSerializerKind::Int => JValue::Int(wire_to_int(v)),
                crate::vm::object::PrimitiveSerializerKind::Long => JValue::Long(wire_to_long(v)),
            });
        }
        _ => {}
    }

    // Message-typed field: hand the embedded bytes to the child serializer.
    let bytes = match v {
        WireValue::Bytes(b) => b.clone(),
        _ => return Err(npe(vm)),
    };
    let child_decoder = proto_child_decoder(vm, bytes)?;
    invoke_deserialize(vm, serializer, child_decoder)
}

/// `CompositeDecoder.decodeSerializableElement(descriptor, index,
/// serializer, previous)` for protobuf.
pub(crate) fn p_decode_serializable_element(vm: &mut Vm, args: &[JValue]) -> PR {
    let serializer = args[3];

    if std::env::var("DEXVM_TRACE").is_ok() {
        let kind = match payload(vm, serializer) {
            Some(Native::ArrayListSerializer { .. }) => "list",
            Some(Native::PrimitiveSerializer(_)) => "primitive",
            Some(Native::JsonElementSerializer) => "jsonelement",
            _ => "message/other",
        };
        eprintln!("PROTO serializable element kind={kind}");
    }

    // Repeated field driven by ArrayListSerializer: consume all consecutive
    // entries sharing the pending field number.
    if matches!(
        payload(vm, serializer),
        Some(Native::ArrayListSerializer { .. })
    ) {
        return proto_list_deserialize_inner(vm, args[0], {
            match payload(vm, serializer) {
                Some(Native::ArrayListSerializer { child }) => *child,
                _ => return Err(npe(vm)),
            }
        });
    }

    // Scalar builtin: convert the pending value directly.
    if matches!(
        payload(vm, serializer),
        Some(Native::PrimitiveSerializer(_))
    ) {
        let Some((_, v)) = take_current(vm, args[0]) else {
            return Err(npe(vm));
        };
        return wire_value_as(vm, &v, serializer);
    }

    // Message-typed field: recurse with a child decoder over its bytes.
    let Some((_, v)) = take_current(vm, args[0]) else {
        return Err(npe(vm));
    };
    let bytes = match v {
        WireValue::Bytes(b) => b,
        _ => return Err(npe(vm)),
    };
    let child_decoder = proto_child_decoder(vm, bytes)?;
    invoke_deserialize(vm, serializer, child_decoder)
}

// ---------------------------------------------------------------------------
// okio BufferedSource entry points
// ---------------------------------------------------------------------------

/// Reads the unread bytes of any Source-shaped payload (mirrors okio.rs's
/// private helper).
fn buffered_bytes_of(vm: &Vm, v: JValue) -> Option<Vec<u8>> {
    match payload(vm, v) {
        Some(Native::OkioBuf { bytes, pos }) => Some(bytes[*pos..].to_vec()),
        Some(Native::ByteArrayInputStream { bytes, pos }) => Some(bytes[*pos..].to_vec()),
        _ => bytes_of(vm, v),
    }
}

/// Extension form: `(format, serializer, source)`.
pub(crate) fn pb_decode_buffered_source_static(vm: &mut Vm, args: &[JValue]) -> PR {
    let mut data: Option<Vec<u8>> = None;
    let mut serializer: Option<JValue> = None;
    for v in args {
        if serializer.is_none() {
            // First non-source object arg is the serializer.
            if buffered_bytes_of(vm, *v).is_none() {
                if let JValue::Obj(_) = v {
                    serializer = Some(*v);
                    continue;
                }
            }
        }
        if data.is_none() {
            if let Some(b) = buffered_bytes_of(vm, *v) {
                data = Some(b);
            }
        }
    }

    let Some(data) = data else {
        return Err(npe(vm));
    };
    let Some(serializer) = serializer else {
        return Err(nat_fatal(JvmError::Resolution(
            "decodeFromBufferedSource: no serializer".into(),
        )));
    };

    let fields = parse_wire(&data).map_err(|e| iae(vm, format!("invalid protobuf: {e}")))?;
    let decoder = alloc(
        vm,
        "Lkotlinx/serialization/protobuf/ProtoDecoder;",
        Native::ProtoDecoder {
            fields,
            cursor: 0,
            cur_field: None,
            cur_val: None,
        },
    )?;
    invoke_deserialize(vm, serializer, decoder)
}

/// Instance form on ProtoBuf/Companion: `(this, serializer, source)`.
pub(crate) fn pb_decode_buffered_source_instance(vm: &mut Vm, args: &[JValue]) -> PR {
    pb_decode_buffered_source_static(vm, args)
}

// ---------------------------------------------------------------------------
// ProtoBuf entry points
// ---------------------------------------------------------------------------

/// Shared implementation: locate the byte payload and the serializer among
/// the arguments (handles instance, companion and `$default` synthetic call
/// shapes), then run the guest serializer over a fresh decoder.
///
/// Serializer selection rule: the first object argument whose payload is
/// NOT `Opaque` — receivers (`ProtoBuf`/companion shims) carry `Opaque`,
/// while guest-generated serializers have none. Byte payloads are matched
/// separately.
fn pb_decode(vm: &mut Vm, args: &[JValue]) -> PR {
    let mut bytes: Option<Vec<u8>> = None;
    let mut serializer: Option<JValue> = None;

    for v in args {
        if let Some(b) = bytes_of(vm, *v) {
            if !b.is_empty() && bytes.is_none() {
                bytes = Some(b);
            }
            continue;
        }
        if serializer.is_none() {
            if let JValue::Obj(o) = v {
                let opaque = vm
                    .arena
                    .get(*o)
                    .and_then(|obj| obj.native.as_ref())
                    .is_some_and(|n| matches!(n, Native::Opaque));
                if !opaque {
                    serializer = Some(*v);
                }
            }
        }
    }

    let Some(data) = bytes else {
        return Err(npe(vm));
    };
    let Some(serializer) = serializer else {
        return Err(nat_fatal(JvmError::Resolution(
            "ProtoBuf.decodeFromByteArray: no serializer".into(),
        )));
    };

    let fields = parse_wire(&data).map_err(|e| iae(vm, format!("invalid protobuf: {e}")))?;
    if std::env::var("DEXVM_TRACE").is_ok() {
        eprintln!(
            "PROTO pb_decode data_len={} fields={} ser_class={}",
            data.len(),
            fields.len(),
            match payload(vm, serializer) {
                _ => String::new(),
            }
        );
        for (f, v) in &fields {
            let desc = match v {
                WireValue::Varint(i) => format!("varint({i})"),
                WireValue::Fixed32(u) => format!("fixed32({u})"),
                WireValue::Fixed64(b) => format!("fixed64({b})"),
                WireValue::Bytes(b) => {
                    format!("bytes[{}]", b.len())
                }
            };
            eprintln!("PROTO   field {f}: {desc}");
        }
    }
    let decoder = alloc(
        vm,
        "Lkotlinx/serialization/protobuf/ProtoDecoder;",
        Native::ProtoDecoder {
            fields,
            cursor: 0,
            cur_field: None,
            cur_val: None,
        },
    )?;

    invoke_deserialize(vm, serializer, decoder)
}

/// Instance form: `(this, serializer, bytes)`.
pub(crate) fn pb_decode_instance(vm: &mut Vm, args: &[JValue]) -> PR {
    pb_decode(vm, args)
}

/// Companion/static form: `(serializer, bytes)`.
pub(crate) fn pb_decode_static(vm: &mut Vm, args: &[JValue]) -> PR {
    pb_decode(vm, args)
}

/// `encodeToByteArray` — not needed by any keiyoushi source flow we support;
/// fail loudly rather than silently producing wrong bytes.
pub(crate) fn pb_encode_unsupported(_vm: &mut Vm, _args: &[JValue]) -> PR {
    Err(nat_fatal(JvmError::Resolution(
        "ProtoBuf.encodeToByteArray is not supported".into(),
    )))
}

// ---------------------------------------------------------------------------
// native table
// ---------------------------------------------------------------------------

pub(crate) const PROTO_DECODER: &str = "Lkotlinx/serialization/protobuf/ProtoDecoder;";
pub(crate) const PROTO_BUF: &str = "Lkotlinx/serialization/protobuf/ProtoBuf;";

pub(crate) static PROTO_TABLE: &[NativeEntry] = &[
    ne!(
        PROTO_DECODER,
        "beginStructure",
        "(Lkotlinx/serialization/descriptors/SerialDescriptor;)Lkotlinx/serialization/encoding/CompositeDecoder;",
        true,
        p_begin_structure
    ),
    ne!(PROTO_DECODER, "decodeSequentially", "()Z", true, p_decode_sequentially),
    ne!(
        PROTO_DECODER,
        "decodeElementIndex",
        "(Lkotlinx/serialization/descriptors/SerialDescriptor;)I",
        true,
        p_decode_element_index
    ),
    ne!(
        PROTO_DECODER,
        "decodeIntElement",
        "(Lkotlinx/serialization/descriptors/SerialDescriptor;I)I",
        true,
        p_decode_int_element
    ),
    ne!(
        PROTO_DECODER,
        "decodeLongElement",
        "(Lkotlinx/serialization/descriptors/SerialDescriptor;I)J",
        true,
        p_decode_long_element
    ),
    ne!(
        PROTO_DECODER,
        "decodeBooleanElement",
        "(Lkotlinx/serialization/descriptors/SerialDescriptor;I)Z",
        true,
        p_decode_boolean_element
    ),
    ne!(
        PROTO_DECODER,
        "decodeFloatElement",
        "(Lkotlinx/serialization/descriptors/SerialDescriptor;I)F",
        true,
        p_decode_float_element
    ),
    ne!(
        PROTO_DECODER,
        "decodeDoubleElement",
        "(Lkotlinx/serialization/descriptors/SerialDescriptor;I)D",
        true,
        p_decode_double_element
    ),
    ne!(
        PROTO_DECODER,
        "decodeStringElement",
        "(Lkotlinx/serialization/descriptors/SerialDescriptor;I)Ljava/lang/String;",
        true,
        p_decode_string_element
    ),
    ne!(
        PROTO_DECODER,
        "decodeBytesElement",
        "(Lkotlinx/serialization/descriptors/SerialDescriptor;I)[B",
        true,
        p_decode_bytes_element
    ),
    ne!(
        PROTO_DECODER,
        "decodeSerializableElement",
        "(Lkotlinx/serialization/descriptors/SerialDescriptor;ILkotlinx/serialization/DeserializationStrategy;Ljava/lang/Object;)Ljava/lang/Object;",
        true,
        p_decode_serializable_element
    ),
    ne!(
        PROTO_DECODER,
        "endStructure",
        "(Lkotlinx/serialization/descriptors/SerialDescriptor;)V",
        true,
        p_end_structure
    ),
    // ProtoBuf instance + companion entry points. The `$default`-style
    // synthetic signatures carry a trailing (int, Object) mask pair.
    ne!(
        PROTO_BUF,
        "decodeFromByteArray",
        "(Lkotlinx/serialization/DeserializationStrategy;[B)Ljava/lang/Object;",
        true,
        pb_decode_instance
    ),
    ne!(
        PROTO_BUF,
        "decodeFromByteArray$default",
        "(Lkotlinx/serialization/protobuf/ProtoBuf;Lkotlinx/serialization/DeserializationStrategy;[BILjava/lang/Object;)Ljava/lang/Object;",
        false,
        pb_decode_static
    ),
    ne!(
        "Lkotlinx/serialization/protobuf/ProtoBuf$Companion;",
        "decodeFromByteArray",
        "(Lkotlinx/serialization/DeserializationStrategy;[B)Ljava/lang/Object;",
        true,
        pb_decode_static
    ),
    ne!(
        PROTO_BUF,
        "decodeFromBufferedSource",
        "(Lkotlinx/serialization/DeserializationStrategy;Lokio/BufferedSource;)Ljava/lang/Object;",
        true,
        pb_decode_buffered_source_instance
    ),
    ne!(
        "Lkotlinx/serialization/protobuf/ProtoBuf$Companion;",
        "decodeFromBufferedSource",
        "(Lkotlinx/serialization/DeserializationStrategy;Lokio/BufferedSource;)Ljava/lang/Object;",
        true,
        pb_decode_static
    ),
    ne!(
        "Lkotlinx/serialization/protobuf/okio/ProtobufOkioKt;",
        "decodeFromBufferedSource",
        "(Lkotlinx/serialization/BinaryFormat;Lkotlinx/serialization/DeserializationStrategy;Lokio/BufferedSource;)Ljava/lang/Object;",
        false,
        pb_decode_buffered_source_static
    ),
    ne!(
        PROTO_BUF,
        "encodeToByteArray",
        "(Lkotlinx/serialization/SerializationStrategy;Ljava/lang/Object;)[B",
        true,
        pb_encode_unsupported
    ),
];
