// Args: output file, start, end [, "list"] : decompile every function whose entry is in [start,end)
import ghidra.app.decompiler.*;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.*;
import java.io.PrintWriter;

public class DecompRange extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        PrintWriter out = new PrintWriter(args[0]);
        Address s = toAddr(args[1]), e = toAddr(args[2]);
        boolean list = args.length > 3;
        DecompInterface d = new DecompInterface();
        d.openProgram(currentProgram);
        FunctionIterator it = currentProgram.getFunctionManager().getFunctions(s, true);
        while (it.hasNext()) {
            Function f = it.next();
            if (f.getEntryPoint().compareTo(e) >= 0) break;
            if (list) { out.println(f.getEntryPoint() + " " + f.getBody().getNumAddresses()); continue; }
            DecompileResults r = d.decompileFunction(f, 180, monitor);
            out.println("=== " + f.getEntryPoint() + " " + f.getName());
            out.println(r.decompileCompleted() ? r.getDecompiledFunction().getC() : "decompile failed: " + r.getErrorMessage());
            out.flush();
        }
        out.close();
    }
}
