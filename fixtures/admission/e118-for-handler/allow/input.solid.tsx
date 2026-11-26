import { For, Show } from "solid-js";

export function Rows() {
  const [items, setItems] = createSignal(["alpha", "beta"]);
  const [picked, setPicked] = createSignal("");
  return (
    <ul>
      <For each={items()} key={(n) => n}>
        {(n) => (
          <Show when={n.length > 1}>
            <li onClick={() => setPicked(n)}>{n}</li>
          </Show>
        )}
      </For>
    </ul>
  );
}
