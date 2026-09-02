/** Keep writes in order, including after a failed write. */
export function createSaveQueue() {
  let tail: Promise<unknown> = Promise.resolve();
  return function enqueue<T>(write: () => Promise<T>): Promise<T> {
    const result = tail.then(write);
    tail = result.catch(() => undefined);
    return result;
  };
}

/** Only acknowledge the submitted snapshot; never replace newer editor text. */
export function acknowledgeSave<T extends { modified_at: number }>(
  current: T,
  submitted: T,
  saved: T,
): T {
  const unchanged = Object.keys(submitted).every(
    (key) => key === "modified_at" ||
      current[key as keyof T] === submitted[key as keyof T],
  );
  return unchanged ? { ...current, modified_at: saved.modified_at } : current;
}
