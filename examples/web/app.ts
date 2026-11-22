import { loadRows } from "./records.js";

const status = document.getElementById("status")!;
const table = document.querySelector("tbody")!;

try {
    const rows = await loadRows(new URL(window.location.href));
    for (const item of rows) {
        const row = table.insertRow();
        for (const value of [item.id, item.title, item.available ? "In stock" : "Out of stock", item.note ?? "—"]) {
            row.insertCell().textContent = value;
        }
    }
    status.textContent = `Loaded ${rows.length} items from PostgreSQL`;
    status.dataset.state = "ready";
} catch (error) {
    status.dataset.state = "error";
    status.textContent = error instanceof Error ? error.message : String(error);
}
