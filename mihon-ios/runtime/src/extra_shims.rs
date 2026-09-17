use dexvm::vm::object::{JsoupDocRef, Native};
use dexvm::vm::{NatErr, NativeEntry, Vm};
use dexvm::vm::value::JValue;

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
