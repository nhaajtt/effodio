@echo off
rem What llama.cpp's Vulkan backend needs to build, made from source into vendor\vulkan-sdk, so the Vulkan SDK
rem doesn't have to be installed: the Vulkan and SPIR-V headers, the shader compiler (glslc), and an import library
rem for the vulkan-1.dll every graphics driver installs.
rem   scripts\build-vulkan-deps.cmd
setlocal
set ROOT=%~dp0..
set SDK=%ROOT%\vendor\vulkan-sdk
set SRC=%ROOT%\vendor\vulkan-src
set PATH=%ROOT%\vendor\tools-venv\Scripts;%PATH%

call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul || exit /b 1
if not exist "%SRC%" mkdir "%SRC%"
if not exist "%SDK%\bin" mkdir "%SDK%\bin"
if not exist "%SDK%\lib" mkdir "%SDK%\lib"

if not exist "%SRC%\Vulkan-Headers" git -c advice.detachedHead=false clone --quiet --depth 1 https://github.com/KhronosGroup/Vulkan-Headers "%SRC%\Vulkan-Headers" || exit /b 1
cmake -S "%SRC%\Vulkan-Headers" -B "%SRC%\build-headers" -G Ninja -DCMAKE_INSTALL_PREFIX="%SDK%" -DVULKAN_HEADERS_ENABLE_MODULE=OFF >nul || exit /b 1
cmake --install "%SRC%\build-headers" >nul || exit /b 1

if not exist "%SRC%\SPIRV-Headers" git -c advice.detachedHead=false clone --quiet --depth 1 https://github.com/KhronosGroup/SPIRV-Headers "%SRC%\SPIRV-Headers" || exit /b 1
cmake -S "%SRC%\SPIRV-Headers" -B "%SRC%\build-spirv" -G Ninja -DCMAKE_INSTALL_PREFIX="%SDK%" >nul || exit /b 1
cmake --install "%SRC%\build-spirv" >nul || exit /b 1

if not exist "%SRC%\shaderc" git -c advice.detachedHead=false clone --quiet --depth 1 https://github.com/google/shaderc "%SRC%\shaderc" || exit /b 1
if not exist "%SRC%\shaderc\third_party\glslang" (
  pushd "%SRC%\shaderc"
  python utils\git-sync-deps || exit /b 1
  popd
)
cmake -S "%SRC%\shaderc" -B "%SRC%\build-shaderc" -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_COMPILER=cl -DCMAKE_CXX_COMPILER=cl ^
  -DSHADERC_SKIP_TESTS=ON -DSHADERC_SKIP_EXAMPLES=ON -DSHADERC_SKIP_COPYRIGHT_CHECK=ON -DSHADERC_SKIP_INSTALL=ON ^
  -DSPIRV_SKIP_TESTS=ON -DSPIRV_SKIP_EXECUTABLES=ON -DENABLE_GLSLANG_BINARIES=OFF >nul || exit /b 1
cmake --build "%SRC%\build-shaderc" --target glslc_exe || exit /b 1
copy /y "%SRC%\build-shaderc\glslc\glslc.exe" "%SDK%\bin\" >nul || exit /b 1

rem An import library for the system's Vulkan loader, from the names it exports.
echo LIBRARY vulkan-1> "%SRC%\vulkan-1.def"
echo EXPORTS>> "%SRC%\vulkan-1.def"
for /f "skip=19 tokens=4" %%f in ('dumpbin /nologo /exports "%SystemRoot%\System32\vulkan-1.dll"') do echo %%f>> "%SRC%\vulkan-1.def"
lib /nologo /def:"%SRC%\vulkan-1.def" /machine:x64 /out:"%SDK%\lib\vulkan-1.lib" >nul || exit /b 1

echo Vulkan build tools are in %SDK%
