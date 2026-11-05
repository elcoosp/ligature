export function Watcher() {
  const [tick, setTick] = createSignal(0);
  const [log, setLog] = createSignal("");
  createEffect(() => setLog(tick().toString()));
  return <em>{log()}</em>;
}
