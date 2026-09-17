//! injekt (kohesive) DI host shims.

use super::*;

// ---------------------------------------------------------------------------
// injekt DI (kohesive)
// ---------------------------------------------------------------------------

pub(crate) fn injekt_get_injekt(vm: &mut Vm, _args: &[JValue]) -> R {
    alloc(vm, "Luy/kohesive/injekt/api/InjektScope;", Native::Opaque)
}

fn extract_injekt_type_param(sig: &str) -> Option<String> {
    let start = sig.find('<')?;
    let end = sig.rfind('>')?;
    let inner = sig[start + 1..end].trim();
    if inner.is_empty() {
        return None;
    }
    Some(inner.to_string())
}

fn injekt_concrete_desc(vm: &mut Vm, recv: JValue) -> Option<String> {
    let JValue::Obj(o) = recv else {
        return None;
    };
    let class = vm.arena.objects.get(o as usize)?.class;
    let desc_id = vm.classes.get(class as usize)?.descriptor;
    if let Some(t) = vm.injekt_type_of(desc_id) {
        let s = vm.str_of(t).to_string();
        return extract_injekt_type_param(&s).or(Some(s));
    }
    let sig = vm.generic_signature(class)?;
    extract_injekt_type_param(&sig).or(Some(sig))
}

fn type_desc_from_args(vm: &mut Vm, args: &[JValue]) -> Option<String> {
    for v in args.iter().rev() {
        match payload(vm, *v) {
            Some(Native::Type { desc }) if !desc.is_empty() => {
                return Some(desc.clone());
            }
            _ => {}
        }
        if let Some(d) = injekt_concrete_desc(vm, *v) {
            if d != "Ljava/lang/reflect/Type;" && !d.is_empty() {
                return Some(d);
            }
        }
    }
    None
}

/// `InjektFactory.getInstance(Type)` — allocates an instance of the concrete
/// type carried by the `java.lang.reflect.Type` argument (which
/// `FullTypeReference.getType()` fills with the receiver's generic
/// signature). When that Type is opaque (Signature stripped / field unread),
/// peek the caller's next `check-cast` — that's the `as T` on `Injekt.get<T>()`.
/// Application is only the last resort when even that is missing.
pub(crate) fn injekt_get_instance(vm: &mut Vm, args: &[JValue]) -> R {
    let raw = type_desc_from_args(vm, args)
        .or_else(|| vm.peek_caller_check_cast())
        .unwrap_or_else(|| "Landroid/app/Application;".to_string());
    let desc = extract_injekt_type_param(&raw).unwrap_or(raw);
    alloc(vm, &desc, Native::Opaque)
}

pub(crate) fn injekt_full_type_init(_vm: &mut Vm, _args: &[JValue]) -> R {
    Ok(JValue::Null)
}

/// `FullTypeReference.getType()` — reflects the concrete generic type of the
/// receiver subclass. Sources, in order:
/// 1. bytecode-derived injekt registry (`getInstance` result check-casts);
/// 2. dex `Signature` annotation on the anonymous subclass;
/// 3. the caller's next `check-cast` (same as `getInstance`).
pub(crate) fn injekt_full_type_get(vm: &mut Vm, args: &[JValue]) -> R {
    let desc = injekt_concrete_desc(vm, args.first().copied().unwrap_or(JValue::Null))
        .or_else(|| vm.peek_caller_check_cast())
        .unwrap_or_default();
    if desc.is_empty() {
        return alloc(vm, "Ljava/lang/reflect/Type;", Native::Opaque);
    }
    alloc(vm, "Ljava/lang/reflect/Type;", Native::Type { desc })
}

// ---------------------------------------------------------------------------
// injekt native table
// ---------------------------------------------------------------------------

pub(crate) const INJEKT_TABLE: &[NativeEntry] = &[
    ne!(
        "Luy/kohesive/injekt/InjektKt;",
        "getInjekt",
        "()Luy/kohesive/injekt/api/InjektScope;",
        false,
        injekt_get_injekt
    ),
    ne!(
        "Luy/kohesive/injekt/InjektKt;",
        "get",
        "(Ljava/lang/reflect/Type;)Ljava/lang/Object;",
        false,
        injekt_get_instance
    ),
    ne!(
        "Luy/kohesive/injekt/InjektKt;",
        "get",
        "(Luy/kohesive/injekt/api/InjektFactory;Ljava/lang/reflect/Type;)Ljava/lang/Object;",
        false,
        injekt_get_instance
    ),
    ne!(
        "Luy/kohesive/injekt/api/InjektFactory;",
        "getInstance",
        "(Ljava/lang/reflect/Type;)Ljava/lang/Object;",
        true,
        injekt_get_instance
    ),
    ne!(
        "Luy/kohesive/injekt/api/InjektScope;",
        "getInstance",
        "(Ljava/lang/reflect/Type;)Ljava/lang/Object;",
        true,
        injekt_get_instance
    ),
    ne!(
        "Luy/kohesive/injekt/api/InjektScope;",
        "get",
        "(Ljava/lang/reflect/Type;)Ljava/lang/Object;",
        true,
        injekt_get_instance
    ),
    ne!(
        "Luy/kohesive/injekt/api/InjektFactory;",
        "get",
        "(Ljava/lang/reflect/Type;)Ljava/lang/Object;",
        true,
        injekt_get_instance
    ),
    ne!(
        "Luy/kohesive/injekt/api/FullTypeReference;",
        "<init>",
        "()V",
        true,
        injekt_full_type_init
    ),
    ne!(
        "Luy/kohesive/injekt/api/FullTypeReference;",
        "getType",
        "()Ljava/lang/reflect/Type;",
        true,
        injekt_full_type_get
    ),
    ne!(
        "Luy/kohesive/injekt/api/TypeReference;",
        "getType",
        "()Ljava/lang/reflect/Type;",
        true,
        injekt_full_type_get
    ),
];

#[cfg(test)]
mod tests;
