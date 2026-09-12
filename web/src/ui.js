// A very small DOM helper. No framework: the page is a few tables and one form, and §2's
// dependency discipline is not something to abandon the moment the code runs in a tab.

export function h(tag, attrs = {}, ...children) {
  const element = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs ?? {})) {
    if (value === null || value === undefined || value === false) continue;
    if (key === "class") element.className = value;
    else if (key === "text") element.textContent = value;
    else if (key === "html") element.innerHTML = value;
    else if (key.startsWith("on") && typeof value === "function") {
      element.addEventListener(key.slice(2).toLowerCase(), value);
    } else element.setAttribute(key, value === true ? "" : String(value));
  }
  for (const child of children.flat()) {
    if (child === null || child === undefined || child === false) continue;
    element.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return element;
}

export function clear(node) {
  while (node.firstChild) node.firstChild.remove();
  return node;
}

/** A hash, shortened for display but copied in full. */
export function shortHash(value, href) {
  const text = String(value ?? "");
  const label = text.length > 20 ? `${text.slice(0, 10)}…${text.slice(-6)}` : text;
  const node = href
    ? h("a", { class: "hash mono", href, title: text }, label)
    : h("span", { class: "hash mono", title: `${text} — click to copy` }, label);
  if (!href) {
    node.addEventListener("click", () => {
      navigator.clipboard?.writeText(text);
      const original = node.textContent;
      node.textContent = "copied";
      setTimeout(() => { node.textContent = original; }, 700);
    });
  }
  return node;
}

/** Group digits so a u128 is readable without pretending it has decimals. */
export function groupDigits(value) {
  const text = String(value ?? "0");
  return text.replace(/\B(?=(\d{3})+(?!\d))/g, " ");
}

export function panel(title, note, ...children) {
  return h("section", { class: "panel" },
    title ? h("h2", { text: title }) : null,
    note ? h("p", { class: "note", text: note }) : null,
    ...children);
}

export function stat(label, value, small = false) {
  return h("section", { class: "panel stat" },
    h("h2", { text: label }),
    h("div", { class: small ? "value small" : "value", text: value }));
}

export function rows(entries) {
  return h("div", { class: "rows" },
    entries.filter(Boolean).map(([key, value]) =>
      h("div", { class: "row" },
        h("span", { class: "k", text: key }),
        h("span", { class: "v" }, value instanceof Node ? value : String(value)))));
}

export function table(headers, bodyRows) {
  return h("div", { class: "scroll" },
    h("table", {},
      h("thead", {}, h("tr", {}, headers.map((header) =>
        h("th", { class: header.num ? "num" : null, text: header.label ?? header })))),
      h("tbody", {}, bodyRows.length > 0
        ? bodyRows
        : h("tr", {}, h("td", { class: "muted", colspan: headers.length, text: "nothing yet" })))));
}

export function banner(kind, message) {
  return h("div", { class: `banner ${kind}` }, message);
}
