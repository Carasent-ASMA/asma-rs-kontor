export type TokenomicsCsv = { filename: string; content: string; revision: string };

/** Only a complete result for the still-observed selection may reach a device action. */
export class TokenomicsCsvPreparation {
  private scope: string | null = null;
  prepared: TokenomicsCsv | null = null;
  version = 0;

  update(scope: string | null): void {
    if (this.scope !== scope) { this.scope = scope; this.cancel(); }
  }
  cancel(): void { this.version++; this.prepared = null; }
  current(version: number): boolean { return this.scope !== null && this.version === version; }

  async prepare(load: () => Promise<TokenomicsCsv>, revision: string): Promise<TokenomicsCsv | null> {
    this.cancel();
    const version = this.version;
    if (!this.current(version)) return null;
    try {
      const value = await load();
      if (!this.current(version)) return null;
      if (value.revision !== revision) throw new Error("csv_snapshot_changed");
      this.prepared = value;
      return value;
    } catch (error) {
      if (!this.current(version)) return null;
      throw error;
    }
  }
}
