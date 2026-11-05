import { For } from "solid-js";

export function Rows() {
  const [items, setItems] = createSignal(["a", "b"]);
  return (
    <ul>
      <For each={items()}>{(n) => <li>{n}</li>}</For>
    </ul>
  );
}
