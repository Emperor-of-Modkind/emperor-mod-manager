/** Finish setup is available once the user has signed in to Nexus (which also captures the API key) or saved a personal API key. */
export function setupCanFinish(
  hasApiKey: boolean,
  nexusWebsiteSignedIn: boolean,
): boolean {
  return hasApiKey || nexusWebsiteSignedIn;
}
