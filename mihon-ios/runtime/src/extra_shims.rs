use dexvm::dex::insn::{decode_all, Insn};
use dexvm::vm::error::JvmError;
use dexvm::vm::object::{ArrayData, JsonVal, JsoupDocRef, Native};
use dexvm::vm::value::JValue;
use dexvm::vm::{NatErr, NativeEntry, NativeFn, Vm};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const DAY_MS: i64 = 86_400_000;
const ZONE_OFFSET: &str = "Ljava/time/ZoneOffset;";
const ZONE_ID: &str = "Ljava/time/ZoneId;";
const LOCAL_DATE_TIME: &str = "Ljava/time/LocalDateTime;";
const ZONED_DATE_TIME: &str = "Ljava/time/ZonedDateTime;";
const DATE_TIME_FORMATTER: &str = "Ljava/time/format/DateTimeFormatter;";

fn noop(_vm: &mut Vm, _args: &[JValue]) -> Result<JValue, NatErr> {
    Ok(JValue::Null)
}

const PRIMITIVE_CLASSES: &[&str] = &["B", "C", "S", "I", "J", "F", "D", "Z", "V"];

pub fn install(vm: &mut Vm) -> Result<(), String> {
    for entry in EXTRA_NATIVES {
        vm.register_native(*entry).map_err(|e| e.to_string())?;
    }
    // DEX `const-class` on primitives (`C` = char, etc.) goes through
    // ensure_class_by_desc. dexvm has no primitive classes of its own.
    for &desc in PRIMITIVE_CLASSES {
        vm.register_native(NativeEntry {
            class: desc,
            name: "mihonPrim",
            sig: "()V",
            instance: false,
            f: noop,
        })
        .map_err(|e| e.to_string())?;
    }
    patch_zone_offset(vm)?;
    patch_json_object(vm)?;
    patch_localized_string(vm)?;
    patch_android(vm)?;
    patch_kotlin_instant(vm)?;
    register_json_decoders(vm)?;
    crate::host::install(vm)
}

fn patch_android(vm: &mut Vm) -> Result<(), String> {
    // MangaDex latest reads prefs; many 1.6 sources const-class this.
    vm.register_native(NativeEntry {
        class: "Landroid/preference/PreferenceManager;",
        name: "getDefaultSharedPreferences",
        sig: "(Landroid/content/Context;)Landroid/content/SharedPreferences;",
        instance: false,
        f: pref_manager_default,
    })
    .map_err(|e| e.to_string())?;

    // Search path does const-class + sget RELEASE.
    for desc in ["Landroid/os/Build;", "Landroid/os/Build$VERSION;"] {
        vm.register_native(NativeEntry {
            class: desc,
            name: "mihonPrim",
            sig: "()V",
            instance: false,
            f: noop,
        })
        .map_err(|e| e.to_string())?;
    }
    let ver = vm
        .ensure_class_by_desc("Landroid/os/Build$VERSION;")
        .map_err(|e| e.to_string())?;
    let release = vm.alloc_string("14");
    let codename = vm.alloc_string("REL");
    install_static(vm, ver, "RELEASE", "Ljava/lang/String;", release);
    install_static(vm, ver, "CODENAME", "Ljava/lang/String;", codename);
    install_static(vm, ver, "SDK_INT", "I", JValue::Int(34));
    Ok(())
}

fn patch_kotlin_instant(vm: &mut Vm) -> Result<(), String> {
    let instant = vm
        .ensure_class_by_desc("Lkotlin/time/Instant;")
        .map_err(|e| e.to_string())?;
    let companion_cls = vm
        .ensure_class_by_desc("Lkotlin/time/Instant$Companion;")
        .map_err(|e| e.to_string())?;
    let companion = JValue::Obj(
        vm.alloc_instance(companion_cls)
            .map_err(|e| e.to_string())?,
    );
    install_static(
        vm,
        instant,
        "Companion",
        "Lkotlin/time/Instant$Companion;",
        companion,
    );
    Ok(())
}

fn pref_manager_default(vm: &mut Vm, _args: &[JValue]) -> Result<JValue, NatErr> {
    vm.shared_preferences.entry("default".into()).or_default();
    alloc(
        vm,
        "Landroid/content/SharedPreferences;",
        Native::SharedPreferences("default".into()),
    )
}

fn patch_zone_offset(vm: &mut Vm) -> Result<(), String> {
    let zone_id = vm
        .ensure_class_by_desc(ZONE_ID)
        .map_err(|e| e.to_string())?;
    let offset = vm
        .ensure_class_by_desc(ZONE_OFFSET)
        .map_err(|e| e.to_string())?;
    vm.classes[offset as usize].superclass = Some(zone_id);

    let utc = vm
        .alloc_native(ZONE_OFFSET, Native::IntBox(0))
        .map_err(|e| e.to_string())?;
    install_static(vm, offset, "UTC", ZONE_OFFSET, utc);
    install_static(vm, offset, "MIN", ZONE_OFFSET, utc);
    install_static(vm, offset, "MAX", ZONE_OFFSET, utc);
    Ok(())
}

pub(crate) fn install_static(vm: &mut Vm, class: u32, name: &str, ty: &str, value: JValue) {
    let name = vm.intern(name);
    let ty = vm.intern(ty);
    let cl = &mut vm.classes[class as usize];
    if cl.static_fields.contains_key(&(name, ty)) {
        return;
    }
    let off = cl.statics.len() as u32;
    cl.statics.push(value);
    cl.statics_lazy.push(None);
    cl.static_fields.insert((name, ty), (class, off));
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn add_iface(vm: &mut Vm, class: u32, iface: &str) -> Result<(), String> {
    let iid = vm.ensure_class_by_desc(iface).map_err(|e| e.to_string())?;
    let ifaces = &mut vm.classes[class as usize].interfaces;
    if !ifaces.contains(&iid) {
        ifaces.push(iid);
    }
    Ok(())
}

/// kotlinx JsonObject is a Map at the JVM. dexvm's shim doesn't implement
/// Map, so `as? JsonObject` / `mapValues` fall through to emptyMap.
fn patch_json_object(vm: &mut Vm) -> Result<(), String> {
    let json_obj = vm
        .ensure_class_by_desc("Lkotlinx/serialization/json/JsonObject;")
        .map_err(|e| e.to_string())?;
    let json_el = vm
        .ensure_class_by_desc("Lkotlinx/serialization/json/JsonElement;")
        .map_err(|e| e.to_string())?;
    vm.classes[json_obj as usize].superclass = Some(json_el);
    add_iface(vm, json_obj, "Ljava/util/Map;")?;
    add_iface(vm, json_obj, "Ljava/io/Serializable;")?;
    Ok(())
}

/// MangaDex (and similar 1.6 JSON sources) use a custom LocalizedString
/// serializer: `decoder.decodeJsonElement() as? JsonObject` then mapValues.
/// dexvm's JsonObject identity fails that `as?`, so every title is "".
fn patch_localized_string(vm: &mut Vm) -> Result<(), String> {
    let classes = classes_with_const_string(vm, "LocalizedString");
    for desc in classes {
        let cid = vm.ensure_class_by_desc(&desc).map_err(|e| e.to_string())?;
        let has_deser = vm.classes[cid as usize]
            .methods
            .iter()
            .any(|m| vm.str_of(m.name) == "deserialize");
        if !has_deser {
            continue;
        }
        let class = leak(desc);
        vm.register_native(NativeEntry {
            class,
            name: "deserialize",
            sig: "(Lkotlinx/serialization/encoding/Decoder;)Ljava/lang/Object;",
            instance: true,
            f: localized_deserialize,
        })
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn classes_with_const_string(vm: &Vm, needle: &str) -> Vec<String> {
    let mut out = Vec::new();
    for dex in &vm.dexes {
        let Some(sid) = dex.strings.iter().position(|s| s.as_ref() == needle) else {
            continue;
        };
        let sid = sid as u32;
        for class in &dex.classes {
            let Some(cd) = &class.class_data else {
                continue;
            };
            let hit = cd
                .direct_methods
                .iter()
                .chain(cd.virtual_methods.iter())
                .filter_map(|em| em.code.as_ref())
                .any(|code| {
                    let Ok(dec) = decode_all(&code.insns) else {
                        return false;
                    };
                    dec.insns.iter().any(|insn| {
                        matches!(
                            insn,
                            Insn::ConstString(_, s) | Insn::ConstStringJumbo(_, s) if *s == sid
                        )
                    })
                });
            if hit {
                out.push(dex.type_descriptor(class.class_idx).to_string());
            }
        }
    }
    out
}

fn localized_deserialize(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let decoder = args.get(1).copied().unwrap_or(JValue::Null);
    let element = match vm.payload_of(decoder) {
        Some(Native::JsonDecoder { element, .. }) => element,
        Some(Native::Json(_)) => decoder,
        _ => JValue::Null,
    };
    json_to_string_map(vm, element)
}

fn json_to_string_map(vm: &mut Vm, element: JValue) -> Result<JValue, NatErr> {
    let mut out: Vec<(JValue, JValue)> = Vec::new();
    match vm.payload_of(element) {
        Some(Native::Json(JsonVal::Object(entries))) => {
            for (k, v) in entries {
                out.push((vm.alloc_string(&k), vm.alloc_string(&json_text(&v))));
            }
        }
        Some(Native::Json(JsonVal::Array(items))) => {
            for item in items {
                match item {
                    JsonVal::Object(entries) => {
                        for (k, v) in entries {
                            out.push((vm.alloc_string(&k), vm.alloc_string(&json_text(&v))));
                        }
                    }
                    JsonVal::Str(s) => {
                        if out.iter().all(|(k, _)| {
                            !matches!(vm.payload_of(*k), Some(Native::Str(t)) if t == "en")
                        }) {
                            out.push((vm.alloc_string("en"), vm.alloc_string(&s)));
                        }
                    }
                    other => {
                        let text = json_text(&other);
                        if !text.is_empty() {
                            out.push((vm.alloc_string("en"), vm.alloc_string(&text)));
                        }
                    }
                }
            }
        }
        Some(Native::Json(JsonVal::Str(s))) => {
            out.push((vm.alloc_string("en"), vm.alloc_string(&s)));
        }
        Some(Native::Map(entries)) => {
            for (k, v) in entries {
                let key = match vm.payload_of(k) {
                    Some(Native::Str(s)) => s,
                    _ => continue,
                };
                let val = match vm.payload_of(v) {
                    Some(Native::Str(s)) => s,
                    Some(Native::Json(j)) => json_text(&j),
                    _ => continue,
                };
                out.push((vm.alloc_string(&key), vm.alloc_string(&val)));
            }
        }
        _ => {}
    }
    alloc(vm, "Ljava/util/LinkedHashMap;", Native::Map(out))
}

fn json_text(v: &JsonVal) -> String {
    match v {
        JsonVal::Str(s) => s.clone(),
        JsonVal::Int(i) => i.to_string(),
        JsonVal::Double(d) => d.to_string(),
        JsonVal::Bool(b) => b.to_string(),
        JsonVal::Null | JsonVal::Object(_) | JsonVal::Array(_) => String::new(),
    }
}

fn decode_json_element(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let element = match args.first().and_then(|v| vm.payload_of(*v)) {
        Some(Native::JsonDecoder { element, .. }) => element,
        _ => return Ok(JValue::Null),
    };
    match vm.payload_of(element) {
        Some(Native::Json(v)) => {
            let desc = match &v {
                JsonVal::Object(_) => "Lkotlinx/serialization/json/JsonObject;",
                JsonVal::Array(_) => "Lkotlinx/serialization/json/JsonArray;",
                JsonVal::Null => "Lkotlinx/serialization/json/JsonNull;",
                _ => "Lkotlinx/serialization/json/JsonPrimitive;",
            };
            alloc(vm, desc, Native::Json(v))
        }
        _ => Ok(element),
    }
}

pub static EXTRA_NATIVES: &[NativeEntry] = &[
    NativeEntry {
        class: "Lorg/jsoup/Jsoup;",
        name: "parse",
        sig: "(Ljava/io/InputStream;Ljava/lang/String;Ljava/lang/String;)Lorg/jsoup/nodes/Document;",
        instance: false,
        f: jsoup_parse_stream,
    },
    NativeEntry {
        class: "Lorg/jsoup/Jsoup;",
        name: "parse",
        sig: "(Ljava/io/InputStream;Ljava/lang/String;Ljava/lang/String;Lorg/jsoup/parser/Parser;)Lorg/jsoup/nodes/Document;",
        instance: false,
        f: jsoup_parse_stream,
    },
    NativeEntry {
        class: ZONE_OFFSET,
        name: "of",
        sig: "(Ljava/lang/String;)Ljava/time/ZoneOffset;",
        instance: false,
        f: zone_offset_of,
    },
    NativeEntry {
        class: ZONE_OFFSET,
        name: "ofHours",
        sig: "(I)Ljava/time/ZoneOffset;",
        instance: false,
        f: zone_offset_of_hours,
    },
    NativeEntry {
        class: ZONE_OFFSET,
        name: "ofHoursMinutes",
        sig: "(II)Ljava/time/ZoneOffset;",
        instance: false,
        f: zone_offset_of_hours_minutes,
    },
    NativeEntry {
        class: ZONE_OFFSET,
        name: "ofTotalSeconds",
        sig: "(I)Ljava/time/ZoneOffset;",
        instance: false,
        f: zone_offset_of_total_seconds,
    },
    NativeEntry {
        class: ZONE_OFFSET,
        name: "UTC",
        sig: "()Ljava/time/ZoneOffset;",
        instance: false,
        f: zone_offset_utc,
    },
    NativeEntry {
        class: ZONE_OFFSET,
        name: "getTotalSeconds",
        sig: "()I",
        instance: true,
        f: zone_offset_get_total_seconds,
    },
    NativeEntry {
        class: ZONE_OFFSET,
        name: "getId",
        sig: "()Ljava/lang/String;",
        instance: true,
        f: zone_offset_get_id,
    },
    NativeEntry {
        class: ZONE_ID,
        name: "getId",
        sig: "()Ljava/lang/String;",
        instance: true,
        f: zone_offset_get_id,
    },
    NativeEntry {
        class: ZONE_OFFSET,
        name: "toString",
        sig: "()Ljava/lang/String;",
        instance: true,
        f: zone_offset_get_id,
    },
    NativeEntry {
        class: ZONE_OFFSET,
        name: "equals",
        sig: "(Ljava/lang/Object;)Z",
        instance: true,
        f: zone_offset_equals,
    },
    NativeEntry {
        class: ZONE_OFFSET,
        name: "hashCode",
        sig: "()I",
        instance: true,
        f: zone_offset_hash_code,
    },
    NativeEntry {
        class: LOCAL_DATE_TIME,
        name: "now",
        sig: "()Ljava/time/LocalDateTime;",
        instance: false,
        f: ldt_now,
    },
    NativeEntry {
        class: LOCAL_DATE_TIME,
        name: "now",
        sig: "(Ljava/time/ZoneId;)Ljava/time/LocalDateTime;",
        instance: false,
        f: ldt_now,
    },
    NativeEntry {
        class: LOCAL_DATE_TIME,
        name: "minusDays",
        sig: "(J)Ljava/time/LocalDateTime;",
        instance: true,
        f: ldt_minus_days,
    },
    NativeEntry {
        class: LOCAL_DATE_TIME,
        name: "truncatedTo",
        sig: "(Ljava/time/temporal/TemporalUnit;)Ljava/time/LocalDateTime;",
        instance: true,
        f: ldt_truncated_to,
    },
    NativeEntry {
        class: LOCAL_DATE_TIME,
        name: "toInstant",
        sig: "(Ljava/time/ZoneOffset;)Ljava/time/Instant;",
        instance: true,
        f: ldt_to_instant,
    },
    NativeEntry {
        class: ZONED_DATE_TIME,
        name: "format",
        sig: "(Ljava/time/format/DateTimeFormatter;)Ljava/lang/String;",
        instance: true,
        f: zdt_format,
    },
    NativeEntry {
        class: DATE_TIME_FORMATTER,
        name: "withZone",
        sig: "(Ljava/time/ZoneId;)Ljava/time/format/DateTimeFormatter;",
        instance: true,
        f: dtf_with_zone,
    },
    NativeEntry {
        class: "Lkotlinx/serialization/json/internal/StreamingJsonDecoder;",
        name: "decodeJsonElement",
        sig: "()Lkotlinx/serialization/json/JsonElement;",
        instance: true,
        f: decode_json_element,
    },
    NativeEntry {
        class: "Lkotlinx/serialization/json/JsonDecoder;",
        name: "decodeJsonElement",
        sig: "()Lkotlinx/serialization/json/JsonElement;",
        instance: true,
        f: decode_json_element,
    },
    NativeEntry {
        class: "Lkotlinx/coroutines/BuildersKt;",
        name: "async$default",
        sig: "(Lkotlinx/coroutines/CoroutineScope;Lkotlin/coroutines/CoroutineContext;Lkotlinx/coroutines/CoroutineStart;Lkotlin/jvm/functions/Function2;ILjava/lang/Object;)Lkotlinx/coroutines/Deferred;",
        instance: false,
        f: coroutines_async_default,
    },
    NativeEntry {
        class: "Lkotlin/time/Instant$Companion;",
        name: "parseOrNull",
        sig: "(Ljava/lang/CharSequence;)Lkotlin/time/Instant;",
        instance: true,
        f: kotlin_instant_parse_or_null,
    },
    NativeEntry {
        class: "Lkotlin/time/Instant$Companion;",
        name: "parse",
        sig: "(Ljava/lang/CharSequence;)Lkotlin/time/Instant;",
        instance: true,
        f: kotlin_instant_parse_or_null,
    },
    NativeEntry {
        class: "Lapp/cash/quickjs/QuickJs;",
        name: "create",
        sig: "()Lapp/cash/quickjs/QuickJs;",
        instance: false,
        f: quickjs_create,
    },
    NativeEntry {
        class: "Lapp/cash/quickjs/QuickJs;",
        name: "evaluate",
        sig: "(Ljava/lang/String;)Ljava/lang/Object;",
        instance: true,
        f: quickjs_evaluate,
    },
    NativeEntry {
        class: "Lapp/cash/quickjs/QuickJs;",
        name: "close",
        sig: "()V",
        instance: true,
        f: noop,
    },
    NativeEntry {
        class: "Lapp/cash/quickjs/QuickJs;",
        name: "compile",
        sig: "(Ljava/lang/String;Ljava/lang/String;)[B",
        instance: true,
        f: quickjs_compile,
    },
    NativeEntry {
        class: "Lapp/cash/quickjs/QuickJs;",
        name: "execute",
        sig: "([B)Ljava/lang/Object;",
        instance: true,
        f: quickjs_execute,
    },
    NativeEntry {
        class: "Ljava/lang/System;",
        name: "getProperty",
        sig: "(Ljava/lang/String;)Ljava/lang/String;",
        instance: false,
        f: sys_get_property,
    },
    NativeEntry {
        class: "Ljava/lang/System;",
        name: "getProperty",
        sig: "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        instance: false,
        f: sys_get_property,
    },
    NativeEntry {
        class: "Ljava/lang/StringBuilder;",
        name: "append",
        sig: "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
        instance: true,
        f: sb_append_str,
    },
    NativeEntry {
        class: "Ljava/lang/StringBuilder;",
        name: "append",
        sig: "(Ljava/lang/CharSequence;)Ljava/lang/StringBuilder;",
        instance: true,
        f: sb_append_str,
    },
    NativeEntry {
        class: "Leu/kanade/tachiyomi/source/model/SManga;",
        name: "getThumbnailUrl",
        sig: "()Ljava/lang/String;",
        instance: true,
        f: smanga_get_thumbnail,
    },
    NativeEntry {
        class: "Leu/kanade/tachiyomi/source/model/SManga;",
        name: "setThumbnailUrl",
        sig: "(Ljava/lang/String;)V",
        instance: true,
        f: smanga_set_thumbnail,
    },
];

const JSON_DECODERS: &[&str] = &[
    "Lkotlinx/serialization/json/internal/StreamingJsonDecoder;",
    "Lkotlinx/serialization/json/JsonDecoder;",
    "Lkotlinx/serialization/encoding/CompositeDecoder;",
    "Lkotlinx/serialization/encoding/Decoder;",
];

fn register_json_decoders(vm: &mut Vm) -> Result<(), String> {
    let methods: &[(&str, &str, NativeFn)] = &[
        (
            "decodeDoubleElement",
            "(Lkotlinx/serialization/descriptors/SerialDescriptor;I)D",
            decode_double_element,
        ),
        (
            "decodeDoubleElement",
            "(Lkotlinx/serialization/descriptors/SerialDescriptor;)D",
            decode_double_element,
        ),
        (
            "decodeFloatElement",
            "(Lkotlinx/serialization/descriptors/SerialDescriptor;I)F",
            decode_float_element,
        ),
        (
            "decodeFloatElement",
            "(Lkotlinx/serialization/descriptors/SerialDescriptor;)F",
            decode_float_element,
        ),
        (
            "decodeByteElement",
            "(Lkotlinx/serialization/descriptors/SerialDescriptor;I)B",
            decode_int_element,
        ),
        (
            "decodeShortElement",
            "(Lkotlinx/serialization/descriptors/SerialDescriptor;I)S",
            decode_int_element,
        ),
        (
            "decodeCharElement",
            "(Lkotlinx/serialization/descriptors/SerialDescriptor;I)C",
            decode_int_element,
        ),
        ("decodeDouble", "()D", decode_double_value),
        ("decodeFloat", "()F", decode_float_value),
        ("decodeByte", "()B", decode_int_value),
        ("decodeShort", "()S", decode_int_value),
        ("decodeChar", "()C", decode_int_value),
        ("decodeNotNullMark", "()Z", decode_not_null_mark),
        ("decodeNull", "()Ljava/lang/Void;", decode_null_value),
        (
            "decodeEnum",
            "(Lkotlinx/serialization/descriptors/SerialDescriptor;)I",
            decode_int_value,
        ),
        (
            "decodeInline",
            "(Lkotlinx/serialization/descriptors/SerialDescriptor;)Lkotlinx/serialization/encoding/Decoder;",
            decode_inline,
        ),
    ];
    for class in JSON_DECODERS {
        for &(name, sig, f) in methods {
            vm.register_native(NativeEntry {
                class,
                name,
                sig,
                instance: true,
                f,
            })
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn jsoup_parse_stream(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let bytes = stream_bytes(vm, args.first().copied().unwrap_or(JValue::Null));
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let base = match args.get(2).and_then(|v| vm.payload_of(*v)) {
        Some(Native::Str(s)) if !s.is_empty() => Some(s),
        _ => None,
    };
    let mut doc = JsoupDocRef::new(dom_query::Document::from(text));
    doc.base = base;
    let cid = vm
        .ensure_class_by_desc("Lorg/jsoup/nodes/Document;")
        .map_err(NatErr::Fatal)?;
    let id = vm.arena.alloc(cid, Vec::new(), Some(Native::JsoupDoc(doc)));
    Ok(JValue::Obj(id))
}

fn stream_bytes(vm: &Vm, v: JValue) -> Vec<u8> {
    match vm.payload_of(v) {
        Some(Native::ByteArrayInputStream { bytes, pos }) => bytes[pos..].to_vec(),
        Some(Native::OkioBuf { bytes, pos }) => bytes[pos..].to_vec(),
        Some(Native::RespBody(b)) => b,
        Some(Native::Response { body, .. }) => body.unwrap_or_default(),
        Some(Native::RequestBody { data, .. }) => data,
        _ => Vec::new(),
    }
}

fn alloc(vm: &mut Vm, desc: &str, native: Native) -> Result<JValue, NatErr> {
    let cid = vm.ensure_class_by_desc(desc).map_err(NatErr::Fatal)?;
    Ok(JValue::Obj(vm.arena.alloc(cid, Vec::new(), Some(native))))
}

fn jvm_to_nat(e: JvmError) -> NatErr {
    match e {
        JvmError::Uncaught(id) => NatErr::Throw(id),
        other => NatErr::Fatal(other),
    }
}

/// dexvm's async$default swallows lambda errors as null, so MangaDex
/// getMangaUpdate returns SMangaUpdate(manga, null) after a failed chapter parse.
fn coroutines_async_default(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let scope = args.first().copied().unwrap_or(JValue::Null);
    let lambda = args.get(3).copied().unwrap_or(JValue::Null);
    let cont = {
        let cid = vm
            .ensure_class_by_desc("Lkotlin/coroutines/jvm/internal/ContinuationImpl;")
            .map_err(NatErr::Fatal)?;
        JValue::Obj(vm.alloc_instance(cid).map_err(NatErr::Fatal)?)
    };
    let value = vm
        .invoke_virtual_args(
            lambda,
            "invoke",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            vec![scope, cont],
        )
        .map_err(jvm_to_nat)?;
    alloc(
        vm,
        "Lkotlinx/coroutines/Deferred;",
        Native::Deferred {
            value,
            error: JValue::Null,
        },
    )
}

fn kotlin_instant_parse_or_null(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let text = match args.get(1).and_then(|v| vm.payload_of(*v)) {
        Some(Native::Str(s)) => s,
        _ => return Ok(JValue::Null),
    };
    match parse_iso_millis(&text) {
        Some(ms) => alloc(vm, "Lkotlin/time/Instant;", Native::EpochMillis(ms)),
        None => Ok(JValue::Null),
    }
}

fn parse_iso_millis(text: &str) -> Option<i64> {
    let s = text.trim();
    let (date, rest) = s.split_once('T').or_else(|| s.split_once(' '))?;
    let rest = rest.strip_suffix('Z').unwrap_or(rest);
    let time = strip_tz_suffix(rest);
    let mut d = date.split('-');
    let y: i64 = d.next()?.parse().ok()?;
    let m: i64 = d.next()?.parse().ok()?;
    let day: i64 = d.next()?.parse().ok()?;
    let (clock, frac) = time.split_once('.').map_or((time, ""), |v| v);
    let mut c = clock.split(':');
    let h: i64 = c.next()?.parse().ok()?;
    let min: i64 = c.next()?.parse().ok()?;
    let sec: i64 = c.next().unwrap_or("0").parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) || h > 23 || min > 59 || sec > 60 {
        return None;
    }
    let adjusted_year = y - i64::from(m <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = m + if m > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    let fraction_millis = frac
        .bytes()
        .filter(u8::is_ascii_digit)
        .take(3)
        .try_fold((0_i64, 0_u8), |(value, digits), byte| {
            Some((value * 10 + i64::from(byte - b'0'), digits + 1))
        })
        .map(|(value, digits)| value * 10_i64.pow(u32::from(3 - digits)))
        .unwrap_or(0);
    Some((days * 86_400 + h * 3600 + min * 60 + sec) * 1000 + fraction_millis)
}

fn strip_tz_suffix(time: &str) -> &str {
    if let Some(i) = time.rfind(['+', '-']) {
        if i >= 8 {
            return &time[..i];
        }
    }
    time
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn as_i64(v: JValue) -> i64 {
    match v {
        JValue::Long(l) => l,
        JValue::Int(i) => i as i64,
        _ => 0,
    }
}

fn millis_of(vm: &Vm, v: JValue) -> Option<i64> {
    match vm.payload_of(v) {
        Some(Native::EpochMillis(m)) => Some(m),
        _ => None,
    }
}

fn offset_seconds(vm: &Vm, v: JValue) -> i32 {
    match vm.payload_of(v) {
        Some(Native::IntBox(s)) => s,
        Some(Native::LongBox(s)) => s as i32,
        Some(Native::Str(s)) => parse_offset(&s).unwrap_or(0),
        _ => 0,
    }
}

fn parse_offset(raw: &str) -> Option<i32> {
    let s = raw.trim();
    if s.eq_ignore_ascii_case("Z")
        || s.eq_ignore_ascii_case("UTC")
        || s.eq_ignore_ascii_case("GMT")
        || s.eq_ignore_ascii_case("UT")
    {
        return Some(0);
    }
    let (sign, rest) = if let Some(r) = s.strip_prefix('+') {
        (1, r)
    } else if let Some(r) = s.strip_prefix('-') {
        (-1, r)
    } else {
        return None;
    };
    let parts: Vec<&str> = rest.split(':').collect();
    let (h, m, sec) = if parts.len() >= 2 {
        (
            parts[0].parse::<i32>().ok()?,
            parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0),
            parts.get(2).and_then(|p| p.parse().ok()).unwrap_or(0),
        )
    } else if rest.len() == 2 {
        (rest.parse().ok()?, 0, 0)
    } else if rest.len() == 4 {
        (rest[..2].parse().ok()?, rest[2..].parse().ok()?, 0)
    } else if rest.len() == 6 {
        (
            rest[..2].parse().ok()?,
            rest[2..4].parse().ok()?,
            rest[4..].parse().ok()?,
        )
    } else {
        (rest.parse().ok()?, 0, 0)
    };
    Some(sign * (h * 3600 + m * 60 + sec))
}

fn offset_id(secs: i32) -> String {
    if secs == 0 {
        return "Z".into();
    }
    let sign = if secs < 0 { '-' } else { '+' };
    let abs = secs.unsigned_abs();
    format!("{sign}{:02}:{:02}", abs / 3600, (abs % 3600) / 60)
}

fn zone_offset_of(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let text = match args.first().and_then(|v| vm.payload_of(*v)) {
        Some(Native::Str(s)) => s,
        _ => "Z".into(),
    };
    let secs = parse_offset(&text).unwrap_or(0);
    alloc(vm, ZONE_OFFSET, Native::IntBox(secs))
}

fn zone_offset_of_hours(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let hours = args.first().copied().unwrap_or(JValue::Int(0)).as_int();
    alloc(vm, ZONE_OFFSET, Native::IntBox(hours * 3600))
}

fn zone_offset_of_hours_minutes(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let hours = args.first().copied().unwrap_or(JValue::Int(0)).as_int();
    let minutes = args.get(1).copied().unwrap_or(JValue::Int(0)).as_int();
    alloc(vm, ZONE_OFFSET, Native::IntBox(hours * 3600 + minutes * 60))
}

fn zone_offset_of_total_seconds(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let secs = args.first().copied().unwrap_or(JValue::Int(0)).as_int();
    alloc(vm, ZONE_OFFSET, Native::IntBox(secs))
}

fn zone_offset_utc(vm: &mut Vm, _args: &[JValue]) -> Result<JValue, NatErr> {
    alloc(vm, ZONE_OFFSET, Native::IntBox(0))
}

fn zone_offset_get_total_seconds(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    Ok(JValue::Int(
        args.first().map(|v| offset_seconds(vm, *v)).unwrap_or(0),
    ))
}

fn zone_offset_get_id(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let secs = args.first().map(|v| offset_seconds(vm, *v)).unwrap_or(0);
    Ok(vm.alloc_string(&offset_id(secs)))
}

fn zone_offset_equals(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let a = args.first().map(|v| offset_seconds(vm, *v)).unwrap_or(0);
    let b = args.get(1).map(|v| offset_seconds(vm, *v)).unwrap_or(0);
    Ok(JValue::Int(i32::from(a == b)))
}

fn zone_offset_hash_code(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    Ok(JValue::Int(
        args.first().map(|v| offset_seconds(vm, *v)).unwrap_or(0),
    ))
}

fn ldt_now(vm: &mut Vm, _args: &[JValue]) -> Result<JValue, NatErr> {
    alloc(vm, LOCAL_DATE_TIME, Native::EpochMillis(now_millis()))
}

fn ldt_minus_days(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let millis = millis_of(vm, args[0]).unwrap_or(0);
    let days = as_i64(args.get(1).copied().unwrap_or(JValue::Long(0)));
    alloc(
        vm,
        LOCAL_DATE_TIME,
        Native::EpochMillis(millis - days * DAY_MS),
    )
}

fn ldt_truncated_to(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let millis = millis_of(vm, args[0]).unwrap_or(0);
    let unit = match args.get(1).and_then(|v| vm.payload_of(*v)) {
        Some(Native::Str(s)) => s,
        Some(Native::Enum { name, .. }) => name,
        _ => "DAYS".into(),
    };
    let truncated = match unit.as_str() {
        "DAYS" => millis.div_euclid(DAY_MS) * DAY_MS,
        "HOURS" => millis.div_euclid(3_600_000) * 3_600_000,
        "MINUTES" => millis.div_euclid(60_000) * 60_000,
        "SECONDS" => millis.div_euclid(1_000) * 1_000,
        _ => millis,
    };
    alloc(vm, LOCAL_DATE_TIME, Native::EpochMillis(truncated))
}

fn ldt_to_instant(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let millis = millis_of(vm, args[0]).unwrap_or(0);
    alloc(vm, "Ljava/time/Instant;", Native::EpochMillis(millis))
}

fn zdt_format(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let millis = millis_of(vm, args[0]).unwrap_or(0);
    let pattern = match args.get(1).and_then(|v| vm.payload_of(*v)) {
        Some(Native::DateFormatter { pattern, .. }) => pattern,
        Some(Native::Str(s)) => s,
        _ => "yyyy-MM-dd'T'HH:mm:ss".into(),
    };
    Ok(vm.alloc_string(&format_pattern(millis, &pattern)))
}

fn dtf_with_zone(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let pattern = match args.first().and_then(|v| vm.payload_of(*v)) {
        Some(Native::DateFormatter { pattern, .. }) => pattern,
        _ => "yyyy-MM-dd'T'HH:mm:ss".into(),
    };
    alloc(
        vm,
        DATE_TIME_FORMATTER,
        Native::DateFormatter {
            pattern,
            zone: "Z".into(),
        },
    )
}

fn format_pattern(millis: i64, pattern: &str) -> String {
    let (year, month, day, hour, minute, second) = civil_utc(millis);
    let pb = pattern.as_bytes();
    let mut out = String::new();
    let mut pi = 0;
    while pi < pb.len() {
        let c = pb[pi];
        if c == b'\'' {
            pi += 1;
            while pi < pb.len() && pb[pi] != b'\'' {
                out.push(pb[pi] as char);
                pi += 1;
            }
            pi += 1;
            continue;
        }
        let mut run = 1;
        while pi + run < pb.len() && pb[pi + run] == c {
            run += 1;
        }
        match c {
            b'y' | b'Y' => out.push_str(&format!("{:0width$}", year, width = run.max(4))),
            b'M' => out.push_str(&format!("{:0width$}", month, width = run)),
            b'd' => out.push_str(&format!("{:0width$}", day, width = run)),
            b'H' => out.push_str(&format!("{:0width$}", hour, width = run)),
            b'm' => out.push_str(&format!("{:0width$}", minute, width = run)),
            b's' => out.push_str(&format!("{:0width$}", second, width = run)),
            _ => {
                for _ in 0..run {
                    out.push(c as char);
                }
            }
        }
        pi += run;
    }
    out
}

fn civil_utc(millis: i64) -> (i32, u32, u32, u32, u32, u32) {
    let secs = millis.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400) as u32;
    let hour = tod / 3600;
    let minute = (tod % 3600) / 60;
    let second = tod % 60;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = i64::from(yoe) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d, hour, minute, second)
}

fn decoder_json(vm: &Vm, decoder: JValue) -> Option<JsonVal> {
    let element = match vm.payload_of(decoder) {
        Some(Native::JsonDecoder { element, .. }) => element,
        Some(Native::Json(_)) => decoder,
        _ => return None,
    };
    match vm.payload_of(element) {
        Some(Native::Json(v)) => Some(v),
        _ => None,
    }
}

fn json_as_f64(v: &JsonVal) -> f64 {
    match v {
        JsonVal::Double(d) => *d,
        JsonVal::Int(i) => *i as f64,
        JsonVal::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        JsonVal::Str(s) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn member_json(vm: &Vm, args: &[JValue]) -> JsonVal {
    let decoder = args.first().copied().unwrap_or(JValue::Null);
    let Some(root) = decoder_json(vm, decoder) else {
        return JsonVal::Null;
    };
    if args.len() < 3 {
        return root;
    }
    let index = match args.get(2).copied().unwrap_or(JValue::Int(0)) {
        JValue::Int(i) => i,
        JValue::Long(l) => l as i32,
        _ => 0,
    };
    if let Some(Native::SerialDescriptor { elements, .. }) =
        args.get(1).and_then(|d| vm.payload_of(*d))
    {
        if let Some(name) = elements.get(index as usize) {
            if let JsonVal::Object(entries) = &root {
                if let Some((_, v)) = entries.iter().find(|(k, _)| k == name) {
                    return v.clone();
                }
            }
        }
    }
    match root {
        JsonVal::Object(entries) => entries
            .get(index as usize)
            .map(|(_, v)| v.clone())
            .unwrap_or(JsonVal::Null),
        JsonVal::Array(items) => items
            .get(index as usize)
            .cloned()
            .unwrap_or(JsonVal::Null),
        other => other,
    }
}

fn decode_double_element(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    Ok(JValue::Double(json_as_f64(&member_json(vm, args))))
}

fn decode_float_element(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    Ok(JValue::Float(json_as_f64(&member_json(vm, args)) as f32))
}

fn decode_int_element(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    Ok(JValue::Int(json_as_f64(&member_json(vm, args)) as i32))
}

fn decode_double_value(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let v = decoder_json(vm, args.first().copied().unwrap_or(JValue::Null))
        .unwrap_or(JsonVal::Null);
    Ok(JValue::Double(json_as_f64(&v)))
}

fn decode_float_value(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let v = decoder_json(vm, args.first().copied().unwrap_or(JValue::Null))
        .unwrap_or(JsonVal::Null);
    Ok(JValue::Float(json_as_f64(&v) as f32))
}

fn decode_int_value(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let v = decoder_json(vm, args.first().copied().unwrap_or(JValue::Null))
        .unwrap_or(JsonVal::Null);
    Ok(JValue::Int(json_as_f64(&v) as i32))
}

fn decode_not_null_mark(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let v = decoder_json(vm, args.first().copied().unwrap_or(JValue::Null));
    Ok(JValue::Int(i32::from(!matches!(v, Some(JsonVal::Null) | None))))
}

fn decode_null_value(_vm: &mut Vm, _args: &[JValue]) -> Result<JValue, NatErr> {
    Ok(JValue::Null)
}

fn decode_inline(_vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    Ok(args.first().copied().unwrap_or(JValue::Null))
}

pub type JsCallback = unsafe extern "C" fn(*const c_char) -> *mut c_char;

static HOST_JS: Mutex<Option<JsCallback>> = Mutex::new(None);

pub fn set_host_js(cb: Option<JsCallback>) {
    *HOST_JS.lock().expect("js cb lock") = cb;
}

fn call_host_js(src: &str) -> Option<String> {
    let cb = (*HOST_JS.lock().ok()?)?;
    let cstr = CString::new(src.replace('\0', "")).ok()?;
    let raw = unsafe { cb(cstr.as_ptr()) };
    if raw.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe { js_free(raw as *mut c_void) };
    Some(text)
}

extern "C" {
    fn free(ptr: *mut c_void);
}

unsafe fn js_free(ptr: *mut c_void) {
    if !ptr.is_null() {
        free(ptr);
    }
}

fn extract_json_literal(src: &str) -> Option<String> {
    for (open, close) in [('[' as u8, ']' as u8), ('{' as u8, '}' as u8)] {
        let bytes = src.as_bytes();
        let start = bytes.iter().position(|&b| b == open)?;
        let end = bytes.iter().rposition(|&b| b == close)?;
        if end <= start {
            continue;
        }
        let slice = src.get(start..=end)?;
        if serde_json::from_str::<serde_json::Value>(slice).is_ok() {
            return Some(slice.to_string());
        }
    }
    None
}

fn eval_js_source(vm: &mut Vm, src: &str) -> Result<JValue, NatErr> {
    if let Some(out) = call_host_js(src) {
        return js_text_to_jvalue(vm, &out);
    }
    if let Some(lit) = extract_json_literal(src) {
        return js_text_to_jvalue(vm, &lit);
    }
    let trimmed = src.trim().trim_end_matches(';');
    if trimmed.starts_with('"') || trimmed.starts_with('\'') {
        return js_text_to_jvalue(vm, trimmed);
    }
    Ok(JValue::Null)
}

fn js_text_to_jvalue(vm: &mut Vm, text: &str) -> Result<JValue, NatErr> {
    let t = text.trim();
    if t.is_empty() || t == "undefined" || t == "null" {
        return Ok(JValue::Null);
    }
    if t == "true" {
        return Ok(JValue::Int(1));
    }
    if t == "false" {
        return Ok(JValue::Int(0));
    }
    if let Ok(n) = t.parse::<i64>() {
        return Ok(JValue::Long(n));
    }
    if (t.starts_with('[') || t.starts_with('{'))
        && serde_json::from_str::<serde_json::Value>(t).is_ok()
    {
        return Ok(vm.alloc_string(t));
    }
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        if let Ok(s) = serde_json::from_str::<String>(t) {
            return Ok(vm.alloc_string(&s));
        }
    }
    Ok(vm.alloc_string(t))
}

fn quickjs_create(vm: &mut Vm, _args: &[JValue]) -> Result<JValue, NatErr> {
    alloc(vm, "Lapp/cash/quickjs/QuickJs;", Native::Opaque)
}

fn quickjs_evaluate(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let src = match args.get(1).and_then(|v| vm.payload_of(*v)) {
        Some(Native::Str(s)) => s,
        _ => return Ok(JValue::Null),
    };
    eval_js_source(vm, &src)
}

fn quickjs_compile(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let src = match args.get(1).and_then(|v| vm.payload_of(*v)) {
        Some(Native::Str(s)) => s,
        _ => String::new(),
    };
    let bytes: Vec<i8> = src.bytes().map(|b| b as i8).collect();
    let cid = vm.ensure_class_by_desc("[B").map_err(NatErr::Fatal)?;
    Ok(JValue::Obj(vm.arena.alloc(
        cid,
        Vec::new(),
        Some(Native::Array(ArrayData::Byte(bytes))),
    )))
}

fn quickjs_execute(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let src = match args.get(1).and_then(|v| vm.payload_of(*v)) {
        Some(Native::Array(ArrayData::Byte(bs))) => {
            String::from_utf8_lossy(bs.iter().map(|&b| b as u8).collect::<Vec<_>>().as_slice())
                .into_owned()
        }
        Some(Native::Str(s)) => s,
        _ => return Ok(JValue::Null),
    };
    eval_js_source(vm, &src)
}

fn sys_get_property(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let key = match args.first().and_then(|v| vm.payload_of(*v)) {
        Some(Native::Str(s)) => s,
        _ => return Ok(args.get(1).copied().unwrap_or(JValue::Null)),
    };
    let value = match key.as_str() {
        "http.agent" => Some(crate::http::USER_AGENT),
        "java.version" | "java.runtime.version" | "java.vm.version" => Some("17"),
        "java.vm.name" | "java.vendor" => Some("dexvm"),
        "java.specification.version" => Some("17"),
        "java.io.tmpdir" => Some("/tmp"),
        "user.home" => Some("/"),
        "user.dir" => Some("/"),
        "file.encoding" => Some("UTF-8"),
        "os.name" => Some("Linux"),
        "os.arch" => Some("aarch64"),
        "line.separator" => Some("\n"),
        "file.separator" => Some("/"),
        "path.separator" => Some(":"),
        _ => None,
    };
    match value {
        Some(v) => Ok(vm.alloc_string(v)),
        None => Ok(args.get(1).copied().unwrap_or(JValue::Null)),
    }
}

fn sb_append_str(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let this = args.first().copied().unwrap_or(JValue::Null);
    let add = match args.get(1).copied() {
        None | Some(JValue::Null) => "null".into(),
        Some(v) => match vm.payload_of(v) {
            Some(Native::Str(s) | Native::StringBuilder(s)) => s,
            _ => "null".into(),
        },
    };
    let JValue::Obj(id) = this else {
        return Err(NatErr::Throw(vm.err_npe()));
    };
    match vm
        .arena
        .objects
        .get_mut(id as usize)
        .and_then(|o| o.native.as_mut())
    {
        Some(Native::StringBuilder(dst)) => dst.push_str(&add),
        _ => return Err(NatErr::Throw(vm.err_npe())),
    }
    Ok(this)
}

fn smanga_native_mut(vm: &mut Vm, v: JValue) -> Option<&mut Native> {
    let JValue::Obj(id) = v else { return None };
    vm.arena.objects.get_mut(id as usize)?.native.as_mut()
}

fn smanga_get_thumbnail(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let url = match args.first().and_then(|v| vm.payload_of(*v)) {
        Some(Native::SManga { thumbnail_url, .. }) => thumbnail_url,
        _ => String::new(),
    };
    Ok(vm.alloc_string(&url))
}

fn smanga_set_thumbnail(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let url = match args.get(1).and_then(|v| vm.payload_of(*v)) {
        Some(Native::Str(s)) => s,
        _ => String::new(),
    };
    if let Some(Native::SManga { thumbnail_url, .. }) =
        smanga_native_mut(vm, args.first().copied().unwrap_or(JValue::Null))
    {
        *thumbnail_url = url;
    }
    Ok(JValue::Null)
}
