// Pure pieces of the provider form: which models a search leaves, and what a
// certificate file holds.

/**
 * The models a search leaves, in the order given. Every word has to appear
 * somewhere in the name, in any order and any case — "sonnet 4" finds
 * `claude-sonnet-4-5`.
 */
export function filterModels(models: string[], query: string): string[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return models;
  return models.filter((model) => {
    const name = model.toLowerCase();
    return words.every((word) => name.includes(word));
  });
}

/** How many certificates a PEM holds — what the form shows instead of the text. */
export function certificateCount(pem: string): number {
  return pem.match(/-----BEGIN CERTIFICATE-----/g)?.length ?? 0;
}

/**
 * A certificate file's contents as PEM. A `.cer` or `.der` is commonly the
 * binary encoding, which the backend does not read, so it is wrapped here;
 * text is taken as it is.
 */
export function pemFromBytes(bytes: Uint8Array): string {
  const text = new TextDecoder().decode(bytes);
  if (text.includes("-----BEGIN")) return text.trim();
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  const base64 = btoa(binary).replace(/.{64}/g, "$&\n").trim();
  return `-----BEGIN CERTIFICATE-----\n${base64}\n-----END CERTIFICATE-----`;
}
