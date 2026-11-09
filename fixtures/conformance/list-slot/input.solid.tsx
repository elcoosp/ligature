import { createSignal, For } from "solid-js";

export function App() {
  const [rows, setRows] = createSignal(["alpha", "beta"]);
  return (
    <main>
      <For each={rows()} key={(t) => t}>
        {(t) => <Slot name="row"><i class="row">{t}</i></Slot>}
      </For>
    </main>
  );
}
