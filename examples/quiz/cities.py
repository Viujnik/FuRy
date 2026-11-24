"""Load the GeoNames cities500 name and population projection into the benchmark."""
import hashlib
from zipfile import ZipFile


async def import_cities(connection, path):
    if path.stat().st_size > 100 * 1024**2:
        raise ValueError("Expected a cities500.zip snapshot under 100 MiB")
    with path.open("rb") as snapshot:
        digest = hashlib.file_digest(snapshot, "sha256").hexdigest()
    await connection.execute("CREATE TABLE bench_scores(id bigint PRIMARY KEY, handle text, score bigint)")
    with ZipFile(path) as archive, archive.open("cities500.txt") as stream:
        if archive.getinfo("cities500.txt").file_size > 256 * 1024**2:
            raise ValueError("GeoNames extract exceeds 256 MiB")

        def records():
            for index, raw in enumerate(stream, 1):
                columns = raw.decode("utf-8").rstrip("\r\n").split("\t")
                if len(columns) != 19:
                    raise ValueError(f"Invalid GeoNames record at line {index}")
                yield index, columns[1], int(columns[14])

        status = await connection.copy_records_to_table("bench_scores", records=records())
    await connection.execute("ANALYZE bench_scores")
    print(f"GeoNames cities500 | {status} | SHA-256 {digest}", flush=True)
