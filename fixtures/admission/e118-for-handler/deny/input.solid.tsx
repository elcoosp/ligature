import { For, Show } from "solid-js";

export function Rows() {
  const [groups, setGroups] = createSignal([["alpha", "beta"]]);
  return (
    <For each={groups()} key={(g) => g.length}>
      {(g) => (
        <For each={g} key={(n) => n}>
          {(n) => (
            <Show when={n.length > 0}>
              <li onClick={() => setGroups([[n]])}>{n}</li>
            </Show>
          )}
        </For>
      )}
    </For>
  );
}
