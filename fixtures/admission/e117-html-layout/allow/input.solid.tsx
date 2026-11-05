export function Rich() {
  const [t, setT] = createSignal("hello");
  return <p>{t()}</p>;
}
