import { For, Show } from "solid-js";

export function Rows() {
  const [on, setOn] = createSignal(true);
  const [items, setItems] = createSignal(["a", "b"]);
  const [picked, setPicked] = createSignal("");
  return (
    <Show when={on()}>
      <For each={items()} key={(n) => n}>
        {(n) => <li onClick={() => setPicked(n)}>{n}</li>}
      </For>
    </Show>
  );
}
