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
// The app shows only its own menus (the tree's, the grid's); the WebView's (Back, Print,
// Inspect, Emoji…) never opens. A part with a menu still gets the event and draws it.
// DevTools in a dev build: F12 or Ctrl+Shift+I.
window.addEventListener("contextmenu", (event) => event.preventDefault());

export default mount(App, { target });
