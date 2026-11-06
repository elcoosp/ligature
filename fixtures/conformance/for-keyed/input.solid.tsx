import { createSignal, For } from "solid-js";

export function Tags() {
  const [tags, setTags] = createSignal(["alpha", "beta", "gamma"]);
  return (
    <ul>
      <For each={tags()} key={(t) => t}>
        {(t) => <li>{t}</li>}
      </For>
    </ul>
  );
}
