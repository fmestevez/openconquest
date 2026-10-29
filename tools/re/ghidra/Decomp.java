// Decompiles the functions containing the given addresses.
// Args: output file, then hex addresses. Prefix an address with "callers:"
// to list the functions calling it instead.
import ghidra.app.decompiler.*;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Reference;
import java.io.PrintWriter;
import java.util.*;

public class Decomp extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        PrintWriter out = new PrintWriter(args[0]);
        DecompInterface d = new DecompInterface();
        d.openProgram(currentProgram);
        for (int i = 1; i < args.length; i++) {
            String a = args[i];
            if (a.startsWith("callers:")) {
                Address addr = toAddr(a.substring(8));
                Set<String> seen = new TreeSet<>();
                for (Reference r : getReferencesTo(addr)) {
                    Function f = getFunctionContaining(r.getFromAddress());
                    seen.add(r.getFromAddress() + " in " + (f == null ? "?" : f.getEntryPoint() + " " + f.getName()));
                }
                out.println("=== callers of " + addr);
                for (String s : seen) out.println("  " + s);
                continue;
            }
            Address addr = toAddr(a);
            Function f = getFunctionContaining(addr);
            if (f == null) {
                out.println("=== no function at " + a);
                continue;
            }
            DecompileResults r = d.decompileFunction(f, 120, monitor);
            out.println("=== " + f.getEntryPoint() + " " + f.getName());
            out.println(r.decompileCompleted() ? r.getDecompiledFunction().getC() : "decompile failed: " + r.getErrorMessage());
        }
        out.close();
    }
}
