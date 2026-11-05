export function Cart() {
  const [tags, setTags] = createSignal(["new"]);
  return (
    <div>
      <button onClick={() => setTags([...tags(), "hot"])}>Tag</button>
      <For each={tags()}>{(t) => <b>{t}</b>}</For>
    </div>
  );
}
