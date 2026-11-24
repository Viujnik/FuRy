import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const { default: Fory, Type } = require("@apache-fory/core");
const protobuf = require("protobufjs");

const proto = await protobuf.load(fileURLToPath(new URL("./leaderboard.proto", import.meta.url)));
const ProtobufLeaderboard = proto.lookupType("quiz.Leaderboard");
const entry = Type.struct({ typeName: "quiz.Entry" }, {
    handle: Type.string(),
    score: Type.int64(),
});
const fory = new Fory({ compatible: true, ref: false });
fory.register(entry);
const { deserialize } = fory.register(Type.struct({ typeName: "quiz.Leaderboard" }, {
    rows: Type.list(entry),
}));

function readRows(message) {
    return message.rows.map(({handle, score}) => ({handle, score: Number(score)}));
}

export default {
    protobuf: (bytes) => readRows(ProtobufLeaderboard.decode(bytes)),
    "apache-fory": (bytes) => readRows(deserialize(bytes)),
};
