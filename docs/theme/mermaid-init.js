// Render the `<pre class="mermaid">` blocks emitted by mdbook-bynk-visuals.
// `mermaid.min.js` (vendored alongside this file) sets `globalThis.mermaid`.
// Loaded via book.toml `additional-js`, after mermaid.min.js.
(function () {
  function run() {
    if (window.mermaid) {
      window.mermaid.initialize({ startOnLoad: false, theme: "neutral" });
      window.mermaid.run({ querySelector: "pre.mermaid" });
    }
  }
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", run);
  } else {
    run();
  }
})();
