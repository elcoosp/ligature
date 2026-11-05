export function Search() {
  const [q, setQ] = createSignal("");
  return <input onInput={(e) => setQ(e.currentTarget.value)} />;
}
