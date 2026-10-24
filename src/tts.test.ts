import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  claimTtsOwner,
  createTtsController,
  isTtsOwner,
  normalizeSpeakText,
  pickEnglishVoice,
  releaseTtsOwner,
  type SpeechSynthesisLike,
  type SpeechSynthesisUtteranceLike,
  type SpeechSynthesisVoiceLike,
} from "./tts";

function voice(
  name: string,
  lang: string,
  localService = true,
): SpeechSynthesisVoiceLike {
  return { name, lang, localService, default: false, voiceURI: name };
}

describe("pickEnglishVoice", () => {
  it("prefers en-US local voice", () => {
    const picked = pickEnglishVoice([
      voice("Alice", "en-GB"),
      voice("Samantha", "en-US"),
      voice("Tingting", "zh-CN"),
    ]);
    expect(picked?.name).toBe("Samantha");
  });

  it("falls back to any en-* voice", () => {
    const picked = pickEnglishVoice([
      voice("Tingting", "zh-CN"),
      voice("Daniel", "en-GB"),
    ]);
    expect(picked?.name).toBe("Daniel");
  });

  it("returns null when no English voice", () => {
    expect(pickEnglishVoice([voice("Tingting", "zh-CN")])).toBeNull();
  });
});

describe("normalizeSpeakText", () => {
  it("collapses whitespace and trims", () => {
    expect(normalizeSpeakText("  hello\n\n  world  ")).toBe("hello world");
  });

  it("returns empty for blank input", () => {
    expect(normalizeSpeakText("   \n\t  ")).toBe("");
  });
});

describe("createTtsController", () => {
  let uttered: SpeechSynthesisUtteranceLike[];
  let synth: SpeechSynthesisLike;

  beforeEach(() => {
    uttered = [];
    synth = {
      speaking: false,
      getVoices: () => [voice("Samantha", "en-US")],
      cancel: vi.fn(() => {
        synth.speaking = false;
      }),
      speak: vi.fn((u: SpeechSynthesisUtteranceLike) => {
        synth.speaking = true;
        uttered.push(u);
        queueMicrotask(() => {
          u.onend?.(new Event("end"));
          synth.speaking = false;
        });
      }),
    };
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("speaks normalized text with English voice and lang", () => {
    const tts = createTtsController({
      synthesis: synth,
      createUtterance: (text) => {
        const u: SpeechSynthesisUtteranceLike = {
          text,
          lang: "",
          rate: 1,
          pitch: 1,
          voice: null,
        };
        return u;
      },
    });

    tts.speak("Hello  world");
    expect(synth.cancel).toHaveBeenCalled();
    expect(uttered).toHaveLength(1);
    expect(uttered[0].text).toBe("Hello world");
    expect(uttered[0].lang).toBe("en-US");
    expect(uttered[0].voice?.name).toBe("Samantha");
    expect(tts.isSpeaking()).toBe(true);
  });

  it("speaks a queue of chunks in order", async () => {
    const tts = createTtsController({
      synthesis: synth,
      createUtterance: (text) => ({ text, lang: "", rate: 1, pitch: 1, voice: null }),
    });

    tts.speakChunks(["First.", "Second."]);
    expect(uttered[0].text).toBe("First.");

    await vi.waitFor(() => expect(uttered).toHaveLength(2));
    expect(uttered[1].text).toBe("Second.");
    await vi.waitFor(() => expect(tts.isSpeaking()).toBe(false));
  });

  it("stop cancels and clears queue", () => {
    const tts = createTtsController({
      synthesis: synth,
      createUtterance: (text) => ({ text, lang: "", rate: 1, pitch: 1, voice: null }),
    });
    tts.speakChunks(["A", "B", "C"]);
    tts.stop();
    expect(synth.cancel).toHaveBeenCalled();
    expect(tts.isSpeaking()).toBe(false);
  });

  it("notifies listeners on start and stop", async () => {
    const tts = createTtsController({
      synthesis: synth,
      createUtterance: (text) => ({ text, lang: "", rate: 1, pitch: 1, voice: null }),
    });
    const states: boolean[] = [];
    tts.subscribe((s) => states.push(s));
    tts.speak("Hi");
    expect(states[states.length - 1]).toBe(true);
    await vi.waitFor(() => expect(states[states.length - 1]).toBe(false));
  });

  it("replacing speech ignores interrupted utterance errors", async () => {
    const delayed: SpeechSynthesisUtteranceLike[] = [];
    synth.speak = vi.fn((u: SpeechSynthesisUtteranceLike) => {
      synth.speaking = true;
      delayed.push(u);
    });

    const tts = createTtsController({
      synthesis: synth,
      createUtterance: (text) => ({ text, lang: "", rate: 1, pitch: 1, voice: null }),
    });

    tts.speak("One");
    expect(delayed).toHaveLength(1);
    tts.speak("Two");
    // simulate cancel interrupt on the first utterance
    delayed[0].onerror?.(new Event("error"));
    expect(tts.isSpeaking()).toBe(true);
    expect(delayed[1].text).toBe("Two");
    delayed[1].onend?.(new Event("end"));
    await vi.waitFor(() => expect(tts.isSpeaking()).toBe(false));
  });

  it("does not notify idle between consecutive speak calls", () => {
    const delayed: SpeechSynthesisUtteranceLike[] = [];
    synth.speak = vi.fn((u: SpeechSynthesisUtteranceLike) => {
      synth.speaking = true;
      delayed.push(u);
    });
    const tts = createTtsController({
      synthesis: synth,
      createUtterance: (text) => ({ text, lang: "", rate: 1, pitch: 1, voice: null }),
    });
    const states: boolean[] = [];
    tts.subscribe((s) => states.push(s));
    tts.speak("One");
    tts.speak("Two");
    expect(states.filter((s) => s === false).length).toBe(1); // initial only
    expect(states[states.length - 1]).toBe(true);
  });
});

describe("tts ownership", () => {
  it("a newer claim steals ownership from an older speaker", () => {
    const a = {};
    const b = {};
    claimTtsOwner(a);
    expect(isTtsOwner(a)).toBe(true);
    claimTtsOwner(b);
    expect(isTtsOwner(a)).toBe(false);
    expect(isTtsOwner(b)).toBe(true);
  });

  it("release only succeeds for the current owner", () => {
    const a = {};
    const b = {};
    claimTtsOwner(a);
    expect(releaseTtsOwner(b)).toBe(false);
    expect(isTtsOwner(a)).toBe(true);
    expect(releaseTtsOwner(a)).toBe(true);
    expect(isTtsOwner(a)).toBe(false);
  });
});
