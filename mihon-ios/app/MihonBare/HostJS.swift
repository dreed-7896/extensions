import Foundation
import JavaScriptCore

enum HostJS {
    static func install() {
        mihon_set_js(mihonSwiftJs)
    }

    static func eval(_ source: UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? {
        guard let source else { return nil }
        let src = String(cString: source)
        let ctx = JSContext()
        ctx?.exceptionHandler = { _, _ in }
        guard let value = ctx?.evaluateScript(src) else { return strdup("null") }
        if value.isUndefined || value.isNull {
            return strdup("null")
        }
        if value.isString {
            return strdup(value.toString() ?? "null")
        }
        ctx?.setObject(value, forKeyedSubscript: "__mihon_out" as NSString)
        let json = ctx?.evaluateScript("JSON.stringify(__mihon_out)")
        return strdup(json?.toString() ?? "null")
    }
}

private let mihonSwiftJs: @convention(c) (UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? = { src in
    HostJS.eval(src)
}
