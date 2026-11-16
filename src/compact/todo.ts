// Compact notch: Todo card. Rows are the Tasks tab's own (panels.ts taskRowFor: cross-off + undo); an inline
// input adds a task (category Work, no time). Task names are user text: textContent only.
import { invoke } from "@tauri-apps/api/core";
import { taskRowFor, type Task } from "../panels";

const node = (tag: string, cls: string, text = "") => Object.assign(document.createElement(tag), { className: cls, textContent: text });

export function renderTodo(el: HTMLElement, s: { tasks: Task[]; onAdded(tasks: Task[]): void }): void {
  const left = node("div", "ct-left");
  const count = node("div", "ct-count");
  count.append(node("b", "", String(s.tasks.length)), node("span", "muted small", " to do"));
  left.append(node("div", "ct-title", "Todo"), count);
  const form = node("form", "ct-add") as HTMLFormElement;
  const input = Object.assign(document.createElement("input"), { placeholder: "Add a task…", maxLength: 100, autocomplete: "off" });
  input.setAttribute("aria-label", "New task");
  form.append(input);
  form.onsubmit = (e) => {
    e.preventDefault();
    const name = input.value.trim();
    if (!name) return;
    invoke<Task[]>("add_task", { name, time: "", category: "Work" }).then((t) => ((input.value = ""), s.onAdded(t)), console.error);
  };
  left.append(form);
  const list = node("div", "ct-list task-list");
  list.append(...(s.tasks.length ? s.tasks.map(taskRowFor) : [node("div", "task-empty muted", "Nothing to do. Nice.")]));
  const wrap = node("div", "ct");
  wrap.append(left, list);
  el.replaceChildren(wrap);
}
