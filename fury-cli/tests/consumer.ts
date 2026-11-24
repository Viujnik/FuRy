import { Message } from "./out/message";
import { Summary } from "./out/summary";

export function update(message: Message): void {
    const native: bigint | null = message.$record.getBigInt("id");
    if (native !== 9007199254740993n || message.note !== null) {
        throw new Error("wire values changed at the generated TypeScript boundary");
    }
    message.$record.setBigInt("id", -9007199254740993n);
    message.text = "updated ✓";
    message.note = "temporary";
    message.note = null;
    message.active = false;
}

export function count(summary: Summary): number {
    summary.label = null;
    return summary.count;
}
