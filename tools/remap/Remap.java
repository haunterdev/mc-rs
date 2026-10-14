import org.objectweb.asm.*;
import org.objectweb.asm.commons.ClassRemapper;
import org.objectweb.asm.commons.Remapper;

import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import java.util.zip.*;

/**
 * Applies an SRG mapping (CL/FD/MD lines) to a jar. Member lookups walk the class
 * hierarchy, so a call site that names a subclass as owner still resolves the
 * mapping declared on the superclass or interface.
 *
 * Usage: java -cp asm.jar;asm-commons.jar;. Remap <in.jar> <mapping.srg> <out.jar>
 */
public class Remap {
    static final Map<String, String> classes = new HashMap<>();
    static final Map<String, String> fields = new HashMap<>();   // owner/name -> name
    static final Map<String, String> methods = new HashMap<>();  // owner/name desc -> name
    static final Map<String, String[]> supers = new HashMap<>(); // obf name -> super + interfaces

    public static void main(String[] args) throws IOException {
        for (String line : Files.readAllLines(Paths.get(args[1]), StandardCharsets.UTF_8)) {
            String[] p = line.trim().split(" ");
            switch (p[0]) {
                case "CL:": classes.put(p[1], p[2]); break;
                case "FD:": fields.put(p[1], p[2].substring(p[2].lastIndexOf('/') + 1)); break;
                case "MD:": methods.put(p[1] + " " + p[2], p[3].substring(p[3].lastIndexOf('/') + 1)); break;
                default: break;
            }
        }
        Map<String, byte[]> entries = new LinkedHashMap<>();
        try (ZipInputStream in = new ZipInputStream(new FileInputStream(args[0]))) {
            for (ZipEntry e; (e = in.getNextEntry()) != null; ) {
                if (e.isDirectory()) continue;
                ByteArrayOutputStream buf = new ByteArrayOutputStream();
                byte[] b = new byte[65536];
                for (int n; (n = in.read(b)) > 0; ) buf.write(b, 0, n);
                entries.put(e.getName(), buf.toByteArray());
            }
        }
        for (Map.Entry<String, byte[]> e : entries.entrySet()) {
            if (!e.getKey().endsWith(".class")) continue;
            ClassReader r = new ClassReader(e.getValue());
            String[] itf = r.getInterfaces();
            String[] s = new String[itf.length + 1];
            s[0] = r.getSuperName();
            System.arraycopy(itf, 0, s, 1, itf.length);
            supers.put(r.getClassName(), s);
        }
        Remapper mapper = new HierarchyRemapper();
        int remapped = 0;
        try (ZipOutputStream out = new ZipOutputStream(new FileOutputStream(args[2]))) {
            for (Map.Entry<String, byte[]> e : entries.entrySet()) {
                String name = e.getKey();
                byte[] data = e.getValue();
                if (name.startsWith("META-INF/") && (name.endsWith(".SF") || name.endsWith(".RSA") || name.endsWith(".DSA"))) continue;
                if (name.endsWith(".class")) {
                    String cls = name.substring(0, name.length() - 6);
                    if (classes.containsKey(cls) || cls.startsWith("net/minecraft/") || cls.indexOf('/') < 0) {
                        ClassReader r = new ClassReader(data);
                        ClassWriter w = new ClassWriter(0);
                        r.accept(new ClassRemapper(w, mapper), 0);
                        data = w.toByteArray();
                        name = mapper.map(cls) + ".class";
                        remapped++;
                    }
                }
                out.putNextEntry(new ZipEntry(name));
                out.write(data);
                out.closeEntry();
            }
        }
        System.out.println("remapped " + remapped + " classes, " + classes.size() + " class mappings");
    }

    static class HierarchyRemapper extends Remapper {
        @Override public String map(String internalName) {
            String m = classes.get(internalName);
            if (m != null) return m;
            int inner = internalName.lastIndexOf('$');
            if (inner > 0) {
                String outer = classes.get(internalName.substring(0, inner));
                if (outer != null) return outer + internalName.substring(inner);
            }
            return internalName;
        }

        @Override public String mapFieldName(String owner, String name, String desc) {
            String m = findField(owner, name, new HashSet<>());
            return m != null ? m : name;
        }

        @Override public String mapMethodName(String owner, String name, String desc) {
            if (name.startsWith("<")) return name;
            String m = findMethod(owner, name, desc, new HashSet<>());
            return m != null ? m : name;
        }

        /** The SAM of a lambda is named by the functional interface it returns. */
        @Override public String mapInvokeDynamicMethodName(String name, String desc) {
            String owner = Type.getReturnType(desc).getInternalName();
            String found = null;
            for (Map.Entry<String, String> e : methods.entrySet()) {
                String k = e.getKey();
                if (k.startsWith(owner + "/" + name + " ")) {
                    if (found != null && !found.equals(e.getValue())) return name;
                    found = e.getValue();
                }
            }
            return found != null ? found : name;
        }

        private String findField(String owner, String name, Set<String> seen) {
            if (owner == null || !seen.add(owner)) return null;
            String m = fields.get(owner + "/" + name);
            if (m != null) return m;
            String[] s = supers.get(owner);
            if (s == null) return null;
            for (String p : s) {
                m = findField(p, name, seen);
                if (m != null) return m;
            }
            return null;
        }

        private String findMethod(String owner, String name, String desc, Set<String> seen) {
            if (owner == null || !seen.add(owner)) return null;
            String m = methods.get(owner + "/" + name + " " + desc);
            if (m != null) return m;
            String[] s = supers.get(owner);
            if (s == null) return null;
            for (String p : s) {
                m = findMethod(p, name, desc, seen);
                if (m != null) return m;
            }
            return null;
        }
    }
}
