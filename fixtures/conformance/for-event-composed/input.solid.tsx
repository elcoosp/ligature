import { For, Show } from "solid-js";

export function PickPanel() {
  const [on, setOn] = createSignal(true);
  const [rows, setRows] = createSignal(["alpha", "beta"]);
  const [picked, setPicked] = createSignal("");
  return (
    <Show when={on()}>
      <ul>
        <For each={rows()} key={(r) => r}>
          {(r) => (
            <li>
              <button onClick={() => setPicked(r)}>{r}</button>
            </li>
          )}
        </For>
      </ul>
    </Show>
  );
}
