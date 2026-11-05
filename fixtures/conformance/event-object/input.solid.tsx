import { createSignal } from "solid-js";

export function Search() {
  const [query, setQuery] = createSignal("");
  return (
    <div class="search">
      <input value={query()} onInput={(e) => setQuery(e.currentTarget.value)} />
      <p>query: {query()}</p>
    </div>
  );
}
