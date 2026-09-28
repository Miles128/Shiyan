import { useCallback, useEffect, useMemo, useState } from "react";
import {
  claimTtsOwner,
  getTts,
  releaseTtsOwner,
  ttsEngineAvailable,
} from "./tts";

export type SpeakTarget =
  | { kind: "article" }
  | { kind: "paragraph"; index: number }
  | { kind: "word" };

/**
 * Shared speech-synthesis state. Unmount cleanup stops playback only when
 * this instance started it — mounting a second consumer (or navigating away
 * after another component stole the speech) never cuts playback off.
 */
export function useTts() {
  const [speaking, setSpeaking] = useState(false);
  const [speakTarget, setSpeakTarget] = useState<SpeakTarget | null>(null);
  const owner = useMemo(() => ({}), []);

  useEffect(() => {
    const tts = getTts();
    return tts.subscribe((isSpeaking) => {
      setSpeaking(isSpeaking);
      if (!isSpeaking) {
        setSpeakTarget(null);
        releaseTtsOwner(owner);
      }
    });
  }, [owner]);

  useEffect(() => {
    return () => {
      if (releaseTtsOwner(owner)) getTts().stop();
    };
  }, [owner]);

  const startSpeak = useCallback(
    (target: SpeakTarget, chunks: string[]) => {
      if (!ttsEngineAvailable()) return false;
      claimTtsOwner(owner);
      setSpeakTarget(target);
      getTts().speakChunks(chunks);
      return true;
    },
    [owner],
  );

  const stopSpeak = useCallback(() => {
    releaseTtsOwner(owner);
    getTts().stop();
    setSpeakTarget(null);
  }, [owner]);

  return useMemo(
    () => ({ speaking, speakTarget, startSpeak, stopSpeak }),
    [speaking, speakTarget, startSpeak, stopSpeak],
  );
}