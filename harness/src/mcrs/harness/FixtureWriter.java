package mcrs.harness;

import java.io.BufferedOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.OutputStream;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.util.zip.GZIPOutputStream;

/**
 * Fixture container, gzip-compressed. Uncompressed layout: the 8 ASCII bytes "MCRSFIX1", then records until EOF.
 * Each record is u16 name length, UTF-8 name, u8 dtype, u32 element count, payload.
 * All integers and payloads are little-endian.
 */
final class FixtureWriter implements AutoCloseable {
    static final int I32 = 1;
    static final int U16 = 2;
    static final int F64 = 3;
    static final int F32 = 4;
    static final int I64 = 5;

    private final OutputStream out;
    private int records;

    FixtureWriter(File file) throws IOException {
        this.out = new BufferedOutputStream(new GZIPOutputStream(new FileOutputStream(file), 1 << 16), 1 << 20);
        this.out.write("MCRSFIX1".getBytes(StandardCharsets.US_ASCII));
    }

    int records() {
        return records;
    }

    private ByteBuffer begin(String name, int dtype, int count, int size) {
        byte[] n = name.getBytes(StandardCharsets.UTF_8);
        ByteBuffer b = ByteBuffer.allocate(2 + n.length + 1 + 4 + count * size).order(ByteOrder.LITTLE_ENDIAN);
        b.putShort((short) n.length);
        b.put(n);
        b.put((byte) dtype);
        b.putInt(count);
        return b;
    }

    private void end(ByteBuffer b) throws IOException {
        out.write(b.array(), 0, b.position());
        records++;
    }

    void i32(String name, int[] a) throws IOException {
        ByteBuffer b = begin(name, I32, a.length, 4);
        for (int v : a) b.putInt(v);
        end(b);
    }

    void u16(String name, char[] a) throws IOException {
        ByteBuffer b = begin(name, U16, a.length, 2);
        for (char v : a) b.putChar(v);
        end(b);
    }

    void f64(String name, double[] a) throws IOException {
        ByteBuffer b = begin(name, F64, a.length, 8);
        for (double v : a) b.putDouble(v);
        end(b);
    }

    void f32(String name, float[] a) throws IOException {
        ByteBuffer b = begin(name, F32, a.length, 4);
        for (float v : a) b.putFloat(v);
        end(b);
    }

    void i64(String name, long[] a) throws IOException {
        ByteBuffer b = begin(name, I64, a.length, 8);
        for (long v : a) b.putLong(v);
        end(b);
    }

    @Override
    public void close() throws IOException {
        out.close();
    }
}
