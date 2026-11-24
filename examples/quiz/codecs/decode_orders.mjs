import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const { default: Fory, Type } = require("@apache-fory/core");
const protobuf = require("protobufjs");
const proto = await new protobuf.Root().load(
    fileURLToPath(new URL("./orders.proto", import.meta.url)), { keepCase: true }
);
const OrderFeed = proto.lookupType("shop.OrderFeed");
const order = Type.struct({ typeName: "shop.Order" }, {
    order_id: Type.int64(),
    placed_at_ms: Type.int64(),
    buyer_name: Type.string(),
    city: Type.string(),
    status: Type.string(),
    currency: Type.string(),
    total_cents: Type.int64(),
    adjustment_cents: Type.int64(),
    item_count: Type.int64(),
    coupon: Type.string().setNullable(true),
    note: Type.string().setNullable(true),
});
const fory = new Fory({ compatible: true, ref: false });
fory.register(order);
const { deserialize } = fory.register(Type.struct({ typeName: "shop.OrderFeed" }, {
    rows: Type.list(order),
}));

function readRows(message) {
    return message.rows.map(row => ({
        order_id: Number(row.order_id),
        placed_at_ms: Number(row.placed_at_ms),
        buyer_name: row.buyer_name,
        city: row.city,
        status: row.status,
        currency: row.currency,
        total_cents: Number(row.total_cents),
        adjustment_cents: Number(row.adjustment_cents),
        item_count: Number(row.item_count),
        coupon: row.coupon ?? null,
        note: row.note ?? null,
    }));
}

export default {
    protobuf: bytes => readRows(OrderFeed.decode(bytes)),
    "apache-fory": bytes => readRows(deserialize(bytes)),
};
