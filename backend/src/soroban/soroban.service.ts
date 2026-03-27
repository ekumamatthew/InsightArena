import { Injectable, Logger } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';

export interface SorobanPredictionResult {
  tx_hash: string;
}

export interface SorobanRpcEvent {
  id: string;
  ledger: number;
  topic: string[];
  value: Record<string, unknown>;
}

export interface SorobanEventsResponse {
  events: SorobanRpcEvent[];
  latestLedger: number;
}

@Injectable()
export class SorobanService {
  private readonly logger = new Logger(SorobanService.name);

  constructor(private readonly configService: ConfigService) {}

  /**
   * Submit a prediction to the Soroban contract, locking the stake on-chain.
   * Returns the transaction hash of the confirmed operation.
   *
   * TODO: Replace stub with real Soroban contract invocation via stellar-sdk.
   */
  submitPrediction(
    userStellarAddress: string,
    marketOnChainId: string,
    chosenOutcome: string,
    stakeAmountStroops: string,
  ): Promise<SorobanPredictionResult> {
    this.logger.log(
      `Soroban submitPrediction: user=${userStellarAddress} market=${marketOnChainId} outcome=${chosenOutcome} stake=${stakeAmountStroops}`,
    );
    // Stub: return a deterministic-looking hash for development/testing.
    const stub = Buffer.from(
      `${marketOnChainId}:${userStellarAddress}:${Date.now()}`,
    )
      .toString('hex')
      .padEnd(64, '0')
      .slice(0, 64);
    return Promise.resolve({ tx_hash: stub });
  }

  async getEvents(fromLedger: number): Promise<SorobanEventsResponse> {
    const rpcUrl = this.configService.get<string>('SOROBAN_RPC_URL');
    const contractId = this.configService.get<string>('SOROBAN_CONTRACT_ID');

    if (!rpcUrl || !contractId) {
      this.logger.warn(
        'SOROBAN_RPC_URL or SOROBAN_CONTRACT_ID is not configured; skipping event poll',
      );
      return { events: [], latestLedger: fromLedger };
    }

    const response = await fetch(rpcUrl, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        jsonrpc: '2.0',
        id: 'insightarena-events',
        method: 'getEvents',
        params: {
          startLedger: fromLedger,
          filters: [{ type: 'contract', contractIds: [contractId] }],
          limit: 200,
        },
      }),
    });

    if (!response.ok) {
      throw new Error(`Soroban RPC error: HTTP ${response.status}`);
    }

    const body = (await response.json()) as {
      error?: { message?: string };
      result?: { events?: unknown[]; latestLedger?: number };
    };

    if (body.error) {
      throw new Error(body.error.message ?? 'Unknown Soroban RPC error');
    }

    const rawEvents = body.result?.events ?? [];
    const latestLedger =
      typeof body.result?.latestLedger === 'number'
        ? body.result.latestLedger
        : fromLedger;

    const events: SorobanRpcEvent[] = rawEvents
      .map((event) => this.normalizeEvent(event))
      .filter((event): event is SorobanRpcEvent => event !== null);

    return { events, latestLedger };
  }

  private normalizeEvent(rawEvent: unknown): SorobanRpcEvent | null {
    if (!rawEvent || typeof rawEvent !== 'object') {
      return null;
    }

    const eventRecord = rawEvent as Record<string, unknown>;
    const id =
      typeof eventRecord.id === 'string'
        ? eventRecord.id
        : `${Date.now()}-${Math.random().toString(16).slice(2, 10)}`;

    const ledger = this.toNumber(eventRecord.ledger);
    if (ledger === null) {
      return null;
    }

    const topic = this.toStringArray(eventRecord.topic ?? eventRecord.topics);
    const value = this.toRecord(eventRecord.value ?? eventRecord.data);

    if (!value) {
      return null;
    }

    return { id, ledger, topic, value };
  }

  private toNumber(value: unknown): number | null {
    if (typeof value === 'number' && Number.isFinite(value)) {
      return value;
    }
    if (typeof value === 'string') {
      const parsed = Number(value);
      return Number.isFinite(parsed) ? parsed : null;
    }
    return null;
  }

  private toStringArray(value: unknown): string[] {
    if (!Array.isArray(value)) {
      return [];
    }

    return value
      .map((item) => {
        if (typeof item === 'string') {
          return item;
        }
        if (item && typeof item === 'object') {
          const obj = item as Record<string, unknown>;
          if (typeof obj.symbol === 'string') {
            return obj.symbol;
          }
          if (typeof obj.value === 'string') {
            return obj.value;
          }
        }
        return null;
      })
      .filter((item): item is string => item !== null);
  }

  private toRecord(value: unknown): Record<string, unknown> | null {
    if (value && typeof value === 'object' && !Array.isArray(value)) {
      return value as Record<string, unknown>;
    }
    return null;
  }
}
