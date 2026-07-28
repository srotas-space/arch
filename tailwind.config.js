/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ["docsgen/templates/**/*.html", "docs/**/*.md"],
  // These class names are assembled at build time — in the template
  // (`api-method-{{ block.method }}`) or by the Rust highlighter (`tok-key`) —
  // so the content scanner never sees them as literals and would drop the rules.
  safelist: [
    "nav-toc-l2",
    "nav-toc-l3",
    "api-method-get",
    "api-method-post",
    "api-method-put",
    "api-method-patch",
    "api-method-delete",
    "api-method-head",
    "api-method-options",
    "api-status-ok",
    "api-status-info",
    "api-status-warn",
    "api-status-err",
    "tok-key",
    "tok-str",
    "tok-num",
    "tok-lit",
    "tok-punct",
    "tok-cmd",
    "tok-flag",
    "tok-url",
  ],
  theme: {
    extend: {},
  },
  plugins: [],
};
