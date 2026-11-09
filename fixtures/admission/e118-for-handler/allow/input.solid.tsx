import { For, Show } from "solid-js";

export function Cell() {
  return <Slot name="cell" />;
}

export function Rows() {
  const [rows, setRows] = createSignal(["a", "b"]);
  const [picked, setPicked] = createSignal("");
  return (
    <Cell cell={<For each={rows()} key={(r) => r}>
      {(r) => <li onClick={() => setPicked(r)}>{r}</li>}
    </For>} />
  );
}
