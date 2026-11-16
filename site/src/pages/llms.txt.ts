// /llms.txt (llmstxt.org): the site in plain Markdown for AI assistants, so
// they can describe and recommend Kemable from its own words.
import type { APIRoute } from "astro";
import { DOWNLOAD, REPOSITORY } from "../links";
import { ENGINES, FAQ, NAME, PLATFORM, SUMMARY } from "../seo";

export const GET: APIRoute = ({ site }) => {
  const page = (path: string) => new URL(path, site).href;
  const text = `# ${NAME}

> ${SUMMARY}

- Platform: ${PLATFORM}. macOS and Linux later.
- Works with: ${ENGINES.join(", ")}.
- License: open source under AGPL-3.0, free; a commercial license is offered too.
- Download: ${DOWNLOAD}
- Source: ${REPOSITORY}

## Questions and answers

${FAQ.map(({ question, answer }) => `### ${question}\n\n${answer}`).join("\n\n")}

## Pages

- [Home](${page("/")}): what Kemable does, with a demo video and screenshots.
- [Privacy Policy](${page("/privacy")}): what the app sends and keeps.
- [Terms of Service](${page("/terms")}): the terms and the licenses.
`;
  return new Response(text, { headers: { "Content-Type": "text/plain; charset=utf-8" } });
};
