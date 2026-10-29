// Decompiles every method of the given vtables.
// Args: output file, then "label=0xVTABLE" pairs.
import ghidra.app.decompiler.*;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import java.io.PrintWriter;
import java.util.*;

public class VTable extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        PrintWriter out = new PrintWriter(args[0]);
        DecompInterface d = new DecompInterface();
        d.openProgram(currentProgram);
        Set<Address> done = new HashSet<>();
        for (int i = 1; i < args.length; i++) {
            String[] kv = args[i].split("=");
            Address vt = toAddr(kv[1]);
            for (int slot = 0; slot < 64; slot++) {
                Address target = toAddr(getInt(vt.add(slot * 4)) & 0xffffffffL);
                Function f = getFunctionAt(target);
                if (f == null) {
                    if (slot > 2) break;
                    continue;
                }
                out.println("=== " + kv[0] + " slot " + slot + " " + f.getEntryPoint() + " " + f.getName());
                if (!done.add(target)) {
                    out.println("(see above)");
                    continue;
                }
                DecompileResults r = d.decompileFunction(f, 60, monitor);
                out.println(r.decompileCompleted() ? r.getDecompiledFunction().getC() : "failed");
            }
        }
        out.close();
    }
}
