import { mount } from "svelte";
import App from "./App.svelte";
// Geist and Geist Mono, bundled so the app never fetches a font (docs/decisions/0012-design-system.md).
import "@fontsource-variable/geist";
import "@fontsource-variable/geist/wght-italic.css";
import "@fontsource-variable/geist-mono";
import "./app.css";

const target = document.getElementById("app");
if (!target) throw new Error("index.html has no #app");
// The components' base rules (font, links, focus ring) hang on .pb-root.
document.body.classList.add("pb-root");

export default mount(App, { target });
