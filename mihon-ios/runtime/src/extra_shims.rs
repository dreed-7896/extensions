use dexvm::dex::insn::{decode_all, Insn};
use dexvm::vm::object::{JsonVal, JsoupDocRef, Native};
use dexvm::vm::value::JValue;
use dexvm::vm::{NatErr, NativeEntry, Vm};
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
    patch_localized_string(vm)
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

fn install_static(vm: &mut Vm, class: u32, name: &str, ty: &str, value: JValue) {
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
];

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
