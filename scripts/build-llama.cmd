@echo off
rem Builds llama.cpp (pinned tag in vendor\llama.cpp, with scripts\llama-no-logits.patch) as shared libraries.
rem Usage: scripts\build-llama.cmd [cpu|vulkan|cuda]
setlocal
set FLAVOR=%1
if "%FLAVOR%"=="" set FLAVOR=cpu
set ROOT=%~dp0..
set SRC=%ROOT%\vendor\llama.cpp
set BUILD=%ROOT%\vendor\llama-build-%FLAVOR%
set OUT=%ROOT%\vendor\llama-%FLAVOR%
set PATH=%ROOT%\vendor\tools-venv\Scripts;%PATH%

call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul || exit /b 1

git -C "%SRC%" apply --check "%ROOT%\scripts\llama-no-logits.patch" 2>nul && git -C "%SRC%" apply "%ROOT%\scripts\llama-no-logits.patch"

set GPU=
if "%FLAVOR%"=="vulkan" set GPU=-DGGML_VULKAN=ON
rem Without the Vulkan SDK installed, the tools scripts\build-vulkan-deps.cmd made are used.
set VK=%ROOT%\vendor\vulkan-sdk
if "%FLAVOR%"=="vulkan" if not defined VULKAN_SDK if exist "%VK%\bin\glslc.exe" (
  set GPU=-DGGML_VULKAN=ON "-DCMAKE_PREFIX_PATH=%VK%" "-DVulkan_INCLUDE_DIR=%VK%\include" "-DVulkan_LIBRARY=%VK%\lib\vulkan-1.lib" "-DVulkan_GLSLC_EXECUTABLE=%VK%\bin\glslc.exe"
)
if "%FLAVOR%"=="cuda" set GPU=-DGGML_CUDA=ON

cmake -S "%SRC%" -B "%BUILD%" -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_COMPILER=cl -DCMAKE_CXX_COMPILER=cl ^
  -DBUILD_SHARED_LIBS=ON -DGGML_BACKEND_DL=ON -DGGML_CPU_ALL_VARIANTS=ON -DGGML_NATIVE=OFF %GPU% ^
  -DLLAMA_BUILD_MTMD=ON -DLLAMA_BUILD_COMMON=OFF -DLLAMA_BUILD_TOOLS=OFF -DLLAMA_BUILD_SERVER=OFF ^
  -DLLAMA_BUILD_EXAMPLES=OFF -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_APP=OFF -DLLAMA_BUILD_UI=OFF ^
  -DLLAMA_OPENSSL=OFF -DLLAMA_CURL=OFF || exit /b 1
cmake --build "%BUILD%" --config Release || exit /b 1

if not exist "%OUT%\bin" mkdir "%OUT%\bin"
if not exist "%OUT%\lib" mkdir "%OUT%\lib"
if not exist "%OUT%\include" mkdir "%OUT%\include"
for /r "%BUILD%" %%f in (*.dll) do copy /y "%%f" "%OUT%\bin\" >nul
for /r "%BUILD%" %%f in (llama.lib mtmd.lib ggml.lib ggml-base.lib) do copy /y "%%f" "%OUT%\lib\" >nul
copy /y "%SRC%\include\llama.h" "%OUT%\include\" >nul
copy /y "%SRC%\ggml\include\*.h" "%OUT%\include\" >nul
copy /y "%SRC%\tools\mtmd\mtmd.h" "%OUT%\include\" >nul
copy /y "%SRC%\tools\mtmd\mtmd-helper.h" "%OUT%\include\" >nul
echo Built llama.cpp (%FLAVOR%) into %OUT%
