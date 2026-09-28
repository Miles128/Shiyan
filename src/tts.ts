export type SpeechSynthesisVoiceLike = {
  name: string;
  lang: string;
  localService: boolean;
  default: boolean;
  voiceURI: string;
};

export type SpeechSynthesisUtteranceLike = {
  text: string;
  lang: string;
  rate: number;
  pitch: number;
  voice: SpeechSynthesisVoiceLike | null;
  onend?: ((ev: Event) => void) | null;
  onerror?: ((ev: Event) => void) | null;
};

export type SpeechSynthesisLike = {
  speaking: boolean;
  getVoices(): SpeechSynthesisVoiceLike[];
  cancel(): void;
  speak(utterance: SpeechSynthesisUtteranceLike): void;
};

export type TtsController = {
  speak(text: string): void;
  speakChunks(chunks: string[]): void;
  stop(): void;
  isSpeaking(): boolean;
  subscribe(listener: (speaking: boolean) => void): () => void;
};

type TtsDeps = {
  synthesis?: SpeechSynthesisLike;
  createUtterance?: (text: string) => SpeechSynthesisUtteranceLike;
};

function browserSynthesis(): SpeechSynthesisLike | null {
  if (typeof window === "undefined" || !window.speechSynthesis) return null;
  return window.speechSynthesis as unknown as SpeechSynthesisLike;
}

/** False on engines without Web Speech (playback would silently do nothing). */
export function ttsEngineAvailable(): boolean {
  return browserSynthesis() !== null;
}

function browserUtterance(text: string): SpeechSynthesisUtteranceLike {
  return new SpeechSynthesisUtterance(text) as unknown as SpeechSynthesisUtteranceLike;
}

export function pickEnglishVoice(
  voices: SpeechSynthesisVoiceLike[],
): SpeechSynthesisVoiceLike | null {
  const english = voices.filter((v) => /^en([-_]|$)/i.test(v.lang));
  if (english.length === 0) return null;

  const score = (v: SpeechSynthesisVoiceLike) => {
    let s = 0;
    if (/^en-US/i.test(v.lang)) s += 4;
    else if (/^en-GB/i.test(v.lang)) s += 3;
    else s += 2;
    if (v.localService) s += 1;
    return s;
  };

  return [...english].sort((a, b) => score(b) - score(a))[0] ?? null;
}

export function normalizeSpeakText(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

export function createTtsController(opts: TtsDeps = {}): TtsController {
  const getSynthesis = () => opts.synthesis ?? browserSynthesis();
  const makeUtterance = opts.createUtterance ?? browserUtterance;

  let queue: string[] = [];
  let active = false;
  let generation = 0;
  const listeners = new Set<(speaking: boolean) => void>();

  function notify() {
    for (const listener of listeners) listener(active);
  }

  function setActive(next: boolean) {
    if (active === next) return;
    active = next;
    notify();
  }

  function speakNext(gen: number) {
    if (gen !== generation) return;

    const synth = getSynthesis();
    if (!synth) {
      queue = [];
      setActive(false);
      return;
    }

    const next = queue.shift();
    if (!next) {
      setActive(false);
      return;
    }

    const utterance = makeUtterance(next);
    const voices = synth.getVoices();
    const voice = pickEnglishVoice(voices);
    utterance.lang = voice?.lang || "en-US";
    utterance.voice = voice;
    utterance.rate = 1;
    utterance.pitch = 1;

    utterance.onend = () => {
      if (gen !== generation) return;
      speakNext(gen);
    };
    utterance.onerror = () => {
      // cancel() often fires "interrupted"; ignore stale generations
      if (gen !== generation) return;
      queue = [];
      setActive(false);
    };

    setActive(true);
    synth.speak(utterance);
  }

  function stop() {
    generation += 1;
    queue = [];
    const synth = getSynthesis();
    synth?.cancel();
    setActive(false);
  }

  function speakChunks(chunks: string[]) {
    const normalized = chunks.map(normalizeSpeakText).filter(Boolean);
    generation += 1;
    queue = [];
    const synth = getSynthesis();
    synth?.cancel();
    if (normalized.length === 0) {
      setActive(false);
      return;
    }
    const gen = generation;
    queue = normalized;
    speakNext(gen);
  }

  return {
    speak(text: string) {
      speakChunks([text]);
    },
    speakChunks,
    stop,
    isSpeaking() {
      return active;
    },
    subscribe(listener) {
      listeners.add(listener);
      listener(active);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

let shared: TtsController | null = null;

/** App-wide TTS instance (cancels previous speech when starting a new one). */
export function getTts(): TtsController {
  if (!shared) shared = createTtsController();
  return shared;
}

let currentOwner: object | null = null;

/**
 * Speech ownership: whoever starts speaking claims it; a component's unmount
 * cleanup stops playback only while it still owns the speech. A newer claim
 * steals ownership, so navigating away from a page that already lost the
 * speech to another component never cuts that speech off.
 */
export function claimTtsOwner(owner: object): void {
  currentOwner = owner;
}

/** True when `owner` started the currently-playing speech. */
export function isTtsOwner(owner: object): boolean {
  return currentOwner === owner;
}

/**
 * Release ownership; returns true when `owner` held it. Use the return value
 * to decide whether an unmount cleanup should stop playback.
 */
export function releaseTtsOwner(owner: object): boolean {
  if (currentOwner !== owner) return false;
  currentOwner = null;
  return true;
}
