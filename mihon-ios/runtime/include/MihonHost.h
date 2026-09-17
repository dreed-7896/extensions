#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct MihonEngine MihonEngine;

/// Host HTTP: `req_json` -> malloc'd JSON `{code,headers,bodyB64}`.
/// Engine frees the returned string with `free(3)`.
typedef char *(*mihon_http_cb)(const char *req_json);

/// Install URLSession/WKWebView HTTP (call once at app start, before open).
void mihon_set_http(mihon_http_cb cb);

/// Load a Mihon/Keiyoushi `.apk`. Caller must `mihon_close`.
/// On failure returns NULL and writes a malloc'd message to `err` (nullable).
MihonEngine *mihon_open_file(const char *apk_path, char **err);

void mihon_close(MihonEngine *engine);

/// JSON-RPC against the loaded extension.
/// `op`: sources | popular | latest | search | details | chapters | pages
/// `args_json`: object, see README.
/// Returns malloc'd JSON (caller `mihon_string_free`). NULL on error.
char *mihon_call(MihonEngine *engine, const char *op, const char *args_json, char **err);

void mihon_string_free(char *s);

#ifdef __cplusplus
}
#endif
