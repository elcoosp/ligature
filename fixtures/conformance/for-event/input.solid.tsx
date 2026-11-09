import { For } from "solid-js";

export function Picks() {
  const [rows, setRows] = createSignal(["alpha", "beta"]);
  const [picked, setPicked] = createSignal("");
  return (
    <ul>
      <For each={rows()} key={(r) => r}>
        {(r) => (
          <li>
            <button onClick={() => setPicked(r)}>{r}</button>
          </li>
        )}
      </For>
    </ul>
  );
}
