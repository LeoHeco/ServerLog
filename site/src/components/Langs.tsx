import TabItem from "@theme/TabItem";
import Tabs from "@theme/Tabs";
import { Children, type FC, isValidElement, type ReactElement, type ReactNode } from "react";
import { LANGS, type Lang } from "../langs.ts";

type Pane = FC<{ children: ReactNode }> & { lang: Lang };

const pane = (lang: Lang): Pane => Object.assign(({ children }: { children: ReactNode }) => children, { lang });

export const Rust = pane("rust");
export const Python = pane("python");
export const TypeScript = pane("typescript");
export const Java = pane("java");
export const Go = pane("go");

const ORDER = Object.keys(LANGS) as Lang[];

export default function Langs({ children }: { children: ReactNode }) {
  const panes = Children.toArray(children)
    .filter((c): c is ReactElement<{ children: ReactNode }, Pane> => isValidElement(c) && typeof c.type !== "string")
    .sort((a, b) => ORDER.indexOf(a.type.lang) - ORDER.indexOf(b.type.lang));
  return (
    <Tabs groupId="lang" queryString="lang" className="langs">
      {panes.map((p) => (
        <TabItem key={p.type.lang} value={p.type.lang} label={LANGS[p.type.lang].label}>
          {p.props.children}
        </TabItem>
      ))}
    </Tabs>
  );
}
