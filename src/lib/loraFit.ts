import type { LoraInfo } from "./types";

/** True when `lora` was trained for a family other than the selected model's.
 *
 *  A hint, never a filter. Family is too coarse to decide compatibility — a
 *  klein-4B and a klein-9B LoRA are both `flux2` and only one will load — and
 *  it is itself guessed, so a model mislabelled by the filename heuristic used
 *  to hide every LoRA the user owned, with no way to override it. Both "" cases
 *  mean "unknown", and unknown is never a mismatch.
 */
export function mismatched(lora: LoraInfo, modelFamily: string): boolean {
  return modelFamily !== "" && lora.family !== "" && lora.family !== modelFamily;
}
