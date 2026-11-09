import { For, Show } from "solid-js";

export function Rows() {
  const [items, setItems] = createSignal(["a", "b"]);
  return (
    <ul>
      <For each={items()}>
        {(n) => (
          <Show when={n.length > 0}>
            <li onClick={() => setItems([n])}>{n}</li>
          </Show>
        )}
      </For>
    </ul>
  );
}
