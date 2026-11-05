import { createSignal, For } from "solid-js";

export function Names() {
  const [names, setNames] = createSignal(["alpha", "beta", "gamma"]);
  return (
    <ul>
      <For each={names()}>{(n) => <li>{n}</li>}</For>
    </ul>
  );
}
