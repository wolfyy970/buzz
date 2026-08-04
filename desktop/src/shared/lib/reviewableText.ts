/**
 * Reject control and formatting characters that can make reviewed agent text
 * differ from what a runtime executes.
 */
export function isProhibitedReviewableTextCharacter(
  character: string,
  allowLayoutControls: boolean,
): boolean {
  const codePoint = character.codePointAt(0);
  if (codePoint === undefined) return false;

  const isControl =
    codePoint <= 0x1f || (codePoint >= 0x7f && codePoint <= 0x9f);
  const isAllowedLayoutControl =
    allowLayoutControls && (codePoint === 0x09 || codePoint === 0x0a);
  if (isControl && !isAllowedLayoutControl) return true;

  return (
    codePoint === 0x2028 ||
    codePoint === 0x2029 ||
    codePoint === 0x00ad ||
    (codePoint >= 0x0600 && codePoint <= 0x0605) ||
    codePoint === 0x034f ||
    codePoint === 0x061c ||
    codePoint === 0x06dd ||
    codePoint === 0x070f ||
    (codePoint >= 0x0890 && codePoint <= 0x0891) ||
    codePoint === 0x08e2 ||
    (codePoint >= 0x115f && codePoint <= 0x1160) ||
    (codePoint >= 0x17b4 && codePoint <= 0x17b5) ||
    (codePoint >= 0x180b && codePoint <= 0x180f) ||
    (codePoint >= 0x200b && codePoint <= 0x200f) ||
    (codePoint >= 0x202a && codePoint <= 0x202e) ||
    (codePoint >= 0x2060 && codePoint <= 0x206f) ||
    codePoint === 0x3164 ||
    (codePoint >= 0xfe00 && codePoint <= 0xfe0f) ||
    codePoint === 0xfeff ||
    codePoint === 0xffa0 ||
    (codePoint >= 0xfff0 && codePoint <= 0xfffb) ||
    codePoint === 0x110bd ||
    codePoint === 0x110cd ||
    (codePoint >= 0x13430 && codePoint <= 0x1343f) ||
    (codePoint >= 0x1bca0 && codePoint <= 0x1bca3) ||
    (codePoint >= 0x1d173 && codePoint <= 0x1d17a) ||
    (codePoint >= 0xe0000 && codePoint <= 0xe0fff)
  );
}

export function isReviewableText(
  value: string,
  allowLayoutControls: boolean,
): boolean {
  return ![...value].some((character) =>
    isProhibitedReviewableTextCharacter(character, allowLayoutControls),
  );
}
