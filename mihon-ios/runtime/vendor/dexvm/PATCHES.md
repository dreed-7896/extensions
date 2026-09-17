# Host patches on dexvm 0.2.0

- `vm/interpret.rs`: coerce mixed numeric types for binop / cmp / if (kotlinx `Double` vs `Int`).
- `vm/native/serialization.rs`: kotlinx polymorphic `type` matching accepts `@SerialName` (`cover_art`) and generated class names (`CoverArtDto`), and matches KClass by class id not object identity.
