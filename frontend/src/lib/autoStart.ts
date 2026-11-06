// The call detector asks the frontend to start a recording (`auto-start-requested`). The
// frontend starts only once its own stop flow has settled: starting during the previous
// meeting's save would clear the transcripts that save is about to write.
export function canAutoStartNow(status: string): boolean {
  return status === 'idle' || status === 'error';
}
