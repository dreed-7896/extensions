use mihon_host::Engine;
use serde_json::json;
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        eprintln!(
            "usage:\n  mihon-host sources <apk>\n  mihon-host popular <apk> [source] [page]\n  mihon-host latest <apk> [source] [page]\n  mihon-host search <apk> <query> [source] [page]\n  mihon-host chapters <apk> <manga-url> [title] [source]\n  mihon-host pages <apk> <chapter-url> [name] [source]"
        );
        return ExitCode::from(2);
    }
    let cmd = args.remove(0);
    if args.is_empty() {
        eprintln!("missing apk path");
        return ExitCode::from(2);
    }
    let apk = args.remove(0);
    let mut engine = match Engine::open_file(&apk) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let result = match cmd.as_str() {
        "sources" => engine.sources().map(|s| json!({ "sources": s })),
        "popular" => {
            let source = parse_usize(args.first(), 0);
            let page = parse_i32(args.get(1), 1);
            engine
                .popular(source, page)
                .map(|(entries, has_next)| json!({ "entries": entries, "hasNext": has_next }))
        }
        "latest" => {
            let source = parse_usize(args.first(), 0);
            let page = parse_i32(args.get(1), 1);
            engine
                .latest(source, page)
                .map(|(entries, has_next)| json!({ "entries": entries, "hasNext": has_next }))
        }
        "search" => {
            if args.is_empty() {
                Err("missing query".into())
            } else {
                let query = args.remove(0);
                let source = parse_usize(args.first(), 0);
                let page = parse_i32(args.get(1), 1);
                engine.search(source, page, &query).map(|(entries, has_next)| {
                    json!({ "entries": entries, "hasNext": has_next })
                })
            }
        }
        "chapters" => {
            if args.is_empty() {
                Err("missing manga url".into())
            } else {
                let url = args.remove(0);
                let title = args.first().cloned().unwrap_or_default();
                let source = parse_usize(args.get(1), 0);
                engine
                    .chapters(source, &url, &title)
                    .map(|chapters| json!({ "chapters": chapters }))
            }
        }
        "pages" => {
            if args.is_empty() {
                Err("missing chapter url".into())
            } else {
                let url = args.remove(0);
                let name = args.first().cloned().unwrap_or_default();
                let source = parse_usize(args.get(1), 0);
                engine
                    .pages(source, &url, &name)
                    .map(|pages| json!({ "pages": pages }))
            }
        }
        other => Err(format!("unknown command {other}")),
    };
    match result {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

fn parse_usize(s: Option<&String>, default: usize) -> usize {
    s.and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn parse_i32(s: Option<&String>, default: i32) -> i32 {
    s.and_then(|v| v.parse().ok()).unwrap_or(default)
}
