/** Background writers drain before the updater replaces the running app. */
const participants = new Set<{ pause: () => Promise<void>; resume: () => void }>();
export function registerUpdateParticipant(participant: { pause: () => Promise<void>; resume: () => void }) {
  participants.add(participant);
  return () => { participants.delete(participant); };
}
export async function pauseForUpdate() {
  const active = [...participants];
  try { await Promise.all(active.map(participant => participant.pause())); }
  catch(error) { for(const participant of active)participant.resume();throw error; }
  return () => { for (const participant of active) participant.resume(); };
}
