export function Watcher() {
  const [tick, setTick] = createSignal(0);
  createEffect(() => console.log(tick()));
  return <em>{tick()}</em>;
}
