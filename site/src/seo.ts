// What search engines and AI assistants read about Kemable: the home page's
// structured data, its questions and answers, and /llms.txt all come from here,
// so they never say different things. Each answer states what the app does
// today; when the app changes, the answer changes with it.
import { DOWNLOAD, REPOSITORY } from "./links";

export const NAME = "Kemable";
export const SUMMARY =
  "A fast, light database client for Windows. Free and open source. It streams millions of rows without a stutter.";
export const ENGINES = ["SQLite", "PostgreSQL", "Neon", "Supabase", "SQL Server", "Redis", "Cloudflare D1", "RabbitMQ", "Firebase"];
export const PLATFORM = "Windows 10 and 11, x64";

export const FAQ: { question: string; answer: string }[] = [
  {
    question: "What is Kemable?",
    answer:
      "Kemable is a free, open-source database client for Windows. One Rust core does the work and a light native window shows it, so it opens fast and stays smooth with millions of rows. It works with SQLite, PostgreSQL (Neon and Supabase too), SQL Server, Redis, Cloudflare D1, RabbitMQ and Firebase.",
  },
  {
    question: "Is Kemable free?",
    answer:
      "Yes. Kemable is open source under the AGPL-3.0 license, free to download and use. A commercial license is offered to anyone who wants to use it without the AGPL's conditions.",
  },
  {
    question: "Does Kemable run on macOS or Linux?",
    answer:
      "Not yet. Kemable 0.1 runs on Windows 10 and 11 (x64). macOS and Linux versions will come later, built on the same core.",
  },
  {
    question: "How does Kemable stay fast with millions of rows?",
    answer:
      "Rows stream from the server into a grid drawn on a canvas, and only the rows on screen are painted. Measured on a 2,000,000-row query: 1.07 s to stream every row, and 0.13 s to sort them by a number.",
  },
  {
    question: "Can I edit table data safely?",
    answer:
      "Yes. Before saving, Kemable shows the exact SQL statements an edit will run, and saves them all or none. Inside a transaction you opened yourself, it saves under a savepoint and never commits or ends your transaction.",
  },
  {
    question: "Does Kemable send my data anywhere?",
    answer:
      "No. Kemable has no account, no server of its own and no telemetry. It connects only to the servers you enter. At launch it asks GitHub once whether a newer version exists, and that check can be turned off.",
  },
  {
    question: "Where does Kemable keep my passwords?",
    answer:
      "In Windows Credential Manager, under your Windows account, never in a file. A saved password is sent only to the host, port and user it was saved for.",
  },
];

/** schema.org data for the home page: the app, and its questions and answers. */
export function structuredData(site: URL) {
  return {
    "@context": "https://schema.org",
    "@graph": [
      {
        "@type": "SoftwareApplication",
        name: NAME,
        description: SUMMARY,
        url: site.href,
        downloadUrl: DOWNLOAD,
        installUrl: DOWNLOAD,
        codeRepository: REPOSITORY,
        applicationCategory: "DeveloperApplication",
        applicationSubCategory: "Database client",
        operatingSystem: PLATFORM,
        license: "https://www.gnu.org/licenses/agpl-3.0.html",
        isAccessibleForFree: true,
        offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
        image: new URL("/og.png", site).href,
        screenshot: new URL("/demo-poster.webp", site).href,
        featureList: [
          "Streams millions of rows into a canvas grid",
          "Edits table data and shows the SQL before saving",
          "SSH tunnels with password or key",
          "Live tabs for PostgreSQL NOTIFY, Redis pub/sub, Firebase Realtime and RabbitMQ queues",
          "Passwords kept in Windows Credential Manager",
          "Command palette (Ctrl+K), light and dark themes",
          `Works with ${ENGINES.join(", ")}`,
        ],
      },
      {
        "@type": "FAQPage",
        mainEntity: FAQ.map(({ question, answer }) => ({
          "@type": "Question",
          name: question,
          acceptedAnswer: { "@type": "Answer", text: answer },
        })),
      },
    ],
  };
}
