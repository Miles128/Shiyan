import Foundation
import Translation

// shiyan-translate — macOS on-device translation helper for 拾言 (Shiyan).
//
// Wraps Apple's Translation framework behind a dumb JSON pipe so the Rust core
// can translate article paragraphs without any network call, API key, or
// bundled model weights.
//
// The programmatic (no-UI) session initializer `init(installedSource:target:)`
// is macOS 26 (Tahoe) or newer — on macOS 15–25 the framework can only be
// reached through a SwiftUI view context. This helper therefore reports
// "unsupported OS" (exit 3) on older systems and the app falls back to its API
// engine.
//
// Usage:
//   shiyan-translate --check                      exit 0 when en→zh-Hans is usable
//   echo '{"texts":["a","b"]}' | shiyan-translate  prints {"translations":["…","…"]}
//
// Exit codes: 0 ok · 1 error · 3 unsupported OS (needs macOS 26+)

struct TranslateIn: Decodable {
    let texts: [String]
}

struct TranslateOut: Encodable {
    let translations: [String]
}

let sourceLanguage = "en"
let targetLanguage = "zh-Hans"

enum HelperError: LocalizedError {
    case languagePackMissing

    var errorDescription: String? {
        switch self {
        case .languagePackMissing:
            return "en→zh-Hans 翻译语言包未安装：请在 系统设置 → 通用 → 语言与地区 → 翻译语言 中添加简体中文与英语"
        }
    }
}

func fail(_ code: Int32, _ message: String) -> Never {
    if !message.isEmpty {
        FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
    }
    exit(code)
}

@available(macOS 26.0, *)
func makeSession() -> TranslationSession {
    // Non-throwing in this SDK: the session object always exists; whether the
    // en→zh-Hans language pack is installed shows up in `isReady` (async).
    TranslationSession(
        installedSource: Locale.Language(identifier: sourceLanguage),
        target: Locale.Language(identifier: targetLanguage)
    )
}

@available(macOS 26.0, *)
func translateAll(_ texts: [String]) async throws -> [String] {
    let session = makeSession()
    guard await session.isReady else {
        throw HelperError.languagePackMissing
    }
    var out: [String] = []
    for text in texts {
        // The framework rejects empty input; pass those through untouched.
        if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            out.append(text)
            continue
        }
        let response = try await session.translate(text)
        out.append(response.targetText)
    }
    return out
}

let arguments = CommandLine.arguments

if arguments.contains("--check") {
    guard #available(macOS 26.0, *) else {
        fail(3, "shiyan-translate: requires macOS 26 (Tahoe) or later")
    }
    let session = makeSession()
    final class Box { var usable = false }
    let box = Box()
    // `TranslationSession` is main-actor isolated: the Task must be able to
    // hop back to the main actor, so the main thread keeps servicing its queue
    // via `dispatchMain()` instead of blocking on a semaphore.
    Task {
        box.usable = await session.isReady
        if box.usable {
            print("ok")
            exit(0)
        }
        fail(1, "shiyan-translate: \(HelperError.languagePackMissing.errorDescription ?? "unavailable")")
    }
    dispatchMain()
}

// Translate mode: JSON on stdin, JSON on stdout. Sequential per text — the
// batch API (`translations(from:)`) adds nothing here and this keeps the
// deployment target at macOS 15 (only the guarded paths need 26).
guard #available(macOS 26.0, *) else {
    fail(3, "shiyan-translate: requires macOS 26 (Tahoe) or later")
}

let inputData = FileHandle.standardInput.readDataToEndOfFile()
guard let input = try? JSONDecoder().decode(TranslateIn.self, from: inputData) else {
    fail(1, "shiyan-translate: invalid stdin JSON, expected {\"texts\":[\"...\"]}")
}

// Main-actor Task + dispatchMain: see the --check branch — blocking the main
// thread with a semaphore would deadlock the session's actor hops.
Task {
    guard #available(macOS 26.0, *) else {
        fail(3, "shiyan-translate: requires macOS 26 (Tahoe) or later")
    }
    do {
        let translations = try await translateAll(input.texts)
        let payload = try JSONEncoder().encode(TranslateOut(translations: translations))
        FileHandle.standardOutput.write(payload)
        exit(0)
    } catch {
        fail(1, "shiyan-translate: \(error.localizedDescription)")
    }
}
dispatchMain()
