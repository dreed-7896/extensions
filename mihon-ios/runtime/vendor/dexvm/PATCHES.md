# Host patches on dexvm 0.2.0

- `dex/insn.rs`: `instance-of` is format 22c — type index is 16-bit. Truncating
  to 8 bits made `it is CoverArtDto` test `Ld;` instead of `Lq0;`, so MangaDex
  (and any reified `firstInstanceOrNull`) dropped covers.
- `vm/interpret.rs`: coerce mixed numeric types for binop / cmp / if (kotlinx `Double` vs `Int`). Bitwise ops coerce Int/Long/Null the same way (ZIP/page decoders).
- `vm/native/serialization.rs`: kotlinx polymorphic `type` matching accepts `@SerialName` (`cover_art`) and generated class names (`CoverArtDto`), and matches KClass by class id not object identity. `decodeElementIndex` matches JSON keys by serial name (skip unknown), `decodeSequentially` is false like real Json, arrays are positional, and `decodeDoubleElement` / `decodeFloatElement` exist on `StreamingJsonDecoder` and `CompositeDecoder` (with and without index).
- `vm/native/keiyoushi.rs`: HttpSource `getPopularManga` / `fetchPopularManga` defaults use `getClient().newCall().awaitSuccess()` so Cloudflare 403 is not parsed as an empty catalog. Default `*Request` joins `baseUrl` + path and forwards source headers. `getPageList` is `fetchPageList.awaitSingle()` like 1.4 HttpSource.
- `keiyoushi.rs`: classic request/parse also rejects non-2xx.
