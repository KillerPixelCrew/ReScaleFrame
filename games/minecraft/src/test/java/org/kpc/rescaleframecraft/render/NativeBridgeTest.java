package org.kpc.rescaleframecraft.render;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

class NativeBridgeTest {
    @Test void stagedJniWritesStatusIntoTheCallerBuffer() {
        var library=Path.of("build/native-resources/native/windows-x64/ReScaleFrame.Minecraft.dll").toAbsolutePath();
        assumeTrue(System.getProperty("os.name").startsWith("Windows")&&Files.isRegularFile(library));
        System.load(library.toString());
        var status=new long[14];Arrays.fill(status,-1);
        NativeBridge.status(0,status);
        assertArrayEquals(new long[14],status,"JNI signature and output layout match the staged DLL");
    }
}
