import { For } from "solid-js";

export function Groups() {
  const [groups, setGroups] = createSignal([["alpha", "beta"]]);
  const [picked, setPicked] = createSignal("");
  return (
    <For each={groups()} key={(g) => g.length}>
      {(g) => (
        <ul>
          <For each={g} key={(n) => n}>
            {(n) => <li onClick={() => setPicked(n)}>{n}</li>}
          </For>
        </ul>
      )}
    </For>
  );
}
