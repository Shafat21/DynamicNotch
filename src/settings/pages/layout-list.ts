// Tabs / Home pages: show-hide toggle + reorder (pointer drag on the handle, or ↑/↓ on a focused row).
import { icon } from "../icons";
import { saveLayout, store, type Layout } from "../store";
import { h, page, toggle } from "../ui";

/** `descs`: one line under each name saying what that tab or card shows. */
export function renderLayoutList(root: HTMLElement, title: string, key: keyof Layout, labels: Record<string, string>, hint: string, descs: Record<string, string> = {}): void {
  const items = () => store.look.layout[key];
  const name = (id: string) => labels[id] ?? id;
  const list = h("ol", { className: "reorder", ariaLabel: title });
  const live = h("p", { className: "sr-only", ariaLive: "polite" });

  const draw = (focusId?: string) => {
    list.replaceChildren(
      ...items().map((it, i) => {
        const grip = h("span", { className: "grip", ariaHidden: "true" }, icon("grip", 16));
        const r = h(
          "li",
          { className: "reorder-row", tabIndex: 0, ariaLabel: `${name(it.id)}, position ${i + 1} of ${items().length}. Arrow up or down to move.` },
          grip,
          h("span", { className: "reorder-text" }, h("span", { className: "reorder-name" }, name(it.id)), ...(descs[it.id] ? [h("span", { className: "reorder-desc" }, descs[it.id])] : [])),
          toggle(`Show ${name(it.id)}`, it.visible, (on) => {
            // by id: a save reply may have replaced the item objects since this row was drawn
            items().forEach((x) => x.id === it.id && (x.visible = on));
            saveLayout();
          }),
        );
        r.dataset.id = it.id;
        r.onkeydown = (e) => {
          const dir = e.key === "ArrowUp" ? -1 : e.key === "ArrowDown" ? 1 : 0;
          if (e.target !== r || !dir || i + dir < 0 || i + dir >= items().length) return;
          e.preventDefault();
          move(i, i + dir);
          live.textContent = `${name(it.id)} moved to position ${i + dir + 1}`;
        };
        grip.onpointerdown = (e) => {
          e.preventDefault();
          grip.setPointerCapture(e.pointerId);
          r.classList.add("dragging");
        };
        grip.onpointermove = (e) => {
          if (!grip.hasPointerCapture(e.pointerId)) return;
          // move the neighbours, never `r`: detaching the captured row would drop pointer capture
          const mid = (o: Element) => o.getBoundingClientRect().top + o.getBoundingClientRect().height / 2;
          for (let o; (o = r.previousElementSibling) && e.clientY < mid(o); ) r.after(o);
          for (let o; (o = r.nextElementSibling) && e.clientY > mid(o); ) r.before(o);
        };
        grip.onlostpointercapture = () => {
          r.classList.remove("dragging");
          const to = [...list.children].indexOf(r);
          if (to !== i) move(i, to);
        };
        return r;
      }),
    );
    if (focusId) list.querySelector<HTMLElement>(`[data-id="${focusId}"]`)?.focus();
  };

  const move = (from: number, to: number) => {
    const next = [...items()];
    const [it] = next.splice(from, 1);
    next.splice(to, 0, it);
    store.look.layout[key] = next; // optimistic; the reply (same order) replaces it
    draw(it.id);
    saveLayout();
  };

  const reset = () => saveLayout({ ...store.look.layout, [key]: [] }).then(() => draw());
  const p = page(title, reset);
  p.append(h("p", { className: "page-hint" }, hint), list, live);
  draw();
  root.append(p);
}
