import type { SpeakTarget } from "../useTts";
import { isPhraseSelection } from "../wordResolve";
import SelectionPopover, { type Popover } from "./SelectionPopover";

export type WordPopoverShellProps = {
  popover: Popover;
  speaking: boolean;
  speakTarget: SpeakTarget | null;
  onSpeakWord: (text: string) => void;
  onAddVocab: () => void;
  onAddPhrase: () => void;
  onClose: () => void;
};

/**
 * Home + Reader share one popover wiring: multi-word selections offer
 * "add to phrase library".
 */
export default function WordPopoverShell({
  popover,
  onAddPhrase,
  ...rest
}: WordPopoverShellProps) {
  const isPhrase =
    popover.source != null && isPhraseSelection(popover.source);
  return (
    <SelectionPopover
      {...rest}
      popover={popover}
      onAddPhrase={isPhrase ? onAddPhrase : undefined}
    />
  );
}
