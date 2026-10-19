import { usePluginData } from "@docusaurus/useGlobalData";
import CodeBlock from "@theme/CodeBlock";
import TabItem from "@theme/TabItem";
import Tabs from "@theme/Tabs";
import { type Examples, LANGS, type Lang } from "../langs.ts";

const repo = "https://github.com/LeoHeco/ServerLog/blob/main/";

type Props = {
  name: string;
  region?: string;
  output?: boolean;
};

export default function Example({ name, region, output = false }: Props) {
  const all = usePluginData("serlog-examples") as Examples;
  const sources = all[name];
  if (!sources) throw new Error(`no example named ${name}`);
  return (
    <Tabs groupId="lang" queryString="lang" className="langs">
      {(Object.keys(LANGS) as Lang[]).map((lang) => {
        const s = sources[lang];
        const text = output ? s.output : region ? s.regions[region] : s.full;
        if (text === undefined) throw new Error(`example ${name} has no region ${region} in ${lang}`);
        return (
          <TabItem key={lang} value={lang} label={LANGS[lang].label}>
            {output ? (
              <CodeBlock language="text">{text}</CodeBlock>
            ) : (
              <CodeBlock language={s.code} title={region ? undefined : <a href={repo + s.file}>{s.file}</a>}>
                {text}
              </CodeBlock>
            )}
          </TabItem>
        );
      })}
    </Tabs>
  );
}
