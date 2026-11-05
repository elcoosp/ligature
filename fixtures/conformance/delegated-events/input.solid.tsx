export function Form() {
  const [name, setName] = createSignal("");
  return (
    <input
      value={name()}
      onInput={() => setName("typed")}
      onClick={() => setName(name().trim())}
    />
  );
}
