import { createSignal, For, Show } from "solid-js";

export function Cell() {
  return <Slot name="cell" />;
}

export function PickList() {
  const [on, setOn] = createSignal(true);
  const [rows, setRows] = createSignal(["alpha", "beta"]);
  const [picked, setPicked] = createSignal("");
  return (
    <Show when={on()}>
      <Cell cell={<For each={rows()} key={(r) => r}>
        {(r) => (
          <li onClick={() => setPicked(r)}>
            <Slot name="row"><i class="pick">{r}</i></Slot>
          </li>
        )}
      </For>} />
    </Show>
  );
}
