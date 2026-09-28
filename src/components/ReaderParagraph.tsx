import type { ReactNode } from "react";
import { memo } from "react";
import Markdown from "react-markdown";

const MARKDOWN_ANNOTATE_TAGS = [
  "p",
  "h1",
  "h2",
  "h3",
  "h4",
  "li",
  "blockquote",
] as const;

export function shouldAnnotateMarkdownTag(tag: string): boolean {
  return (MARKDOWN_ANNOTATE_TAGS as readonly string[]).includes(tag);
}

type Props = {
  text: string;
  paraIndex: number;
  asMarkdown: boolean;
  annotateChildren: (children: ReactNode) => ReactNode;
  zhVisible: boolean;
  zhText?: string;
  translating: boolean;
  visiblePara: boolean;
  paraSpeaking: boolean;
  onTranslate: (index: number) => void;
  onSpeak: (index: number) => void;
};

export default memo(function ReaderParagraph({
  text,
  paraIndex,
  asMarkdown,
  annotateChildren,
  zhVisible,
  zhText,
  translating,
  visiblePara,
  paraSpeaking,
  onTranslate,
  onSpeak,
}: Props) {
  return (
    <div className="para-block">
      <div className="para-gutter">
        <button
          className="para-btn"
          type="button"
          title="翻译本段"
          onClick={() => onTranslate(paraIndex)}
          disabled={translating}
        >
          {translating ? "…" : visiblePara ? "隐" : "译"}
        </button>
        <button
          className={`para-btn${paraSpeaking ? " active" : ""}`}
          type="button"
          title={paraSpeaking ? "停止朗读" : "朗读本段"}
          onClick={() => onSpeak(paraIndex)}
        >
          {paraSpeaking ? "停" : "读"}
        </button>
      </div>
      <div className="para-content">
        {asMarkdown ? (
          <div className="md-preview">
            <Markdown
              components={{
                a: ({ href, children }) => (
                  <a href={href} target="_blank" rel="noreferrer">
                    {children}
                  </a>
                ),
                p: ({ children }) => <p>{annotateChildren(children)}</p>,
                h1: ({ children }) => <h1>{annotateChildren(children)}</h1>,
                h2: ({ children }) => <h2>{annotateChildren(children)}</h2>,
                h3: ({ children }) => <h3>{annotateChildren(children)}</h3>,
                h4: ({ children }) => <h4>{annotateChildren(children)}</h4>,
                li: ({ children }) => <li>{annotateChildren(children)}</li>,
                blockquote: ({ children }) => (
                  <blockquote>{annotateChildren(children)}</blockquote>
                ),
              }}
            >
              {text}
            </Markdown>
          </div>
        ) : (
          <p>{annotateChildren(text)}</p>
        )}
        {zhVisible && zhText && <p className="zh">{zhText}</p>}
      </div>
    </div>
  );
});
