type EmailJob = {
  readonly to: string;
  readonly subject: string;
};

type DeliveryResult =
  | { readonly ok: true }
  | { readonly ok: false; readonly temporary: boolean };

type EntryPoint = {
  register(handler: (job: EmailJob) => Promise<DeliveryResult>): void;
};

declare const httpPost: EntryPoint;
declare const queueConsumer: EntryPoint;
declare const dailySchedule: EntryPoint;
declare const socketMessage: EntryPoint;

declare const mailer: {
  send(job: EmailJob): Promise<DeliveryResult>;
};

async function deliver(job: EmailJob): Promise<DeliveryResult> {
  return mailer.send(job);
}

for (const entry of [
  httpPost,
  queueConsumer,
  dailySchedule,
  socketMessage,
]) {
  entry.register(deliver);
}
