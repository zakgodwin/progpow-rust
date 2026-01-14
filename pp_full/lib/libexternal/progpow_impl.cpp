#include <iostream>
#include <libdevcore/Common.h>
#include "./progpow.h"

#define ERROR_NOT_GPU "Isn't possible found a GPU"

#define MAX_MINERS 4

using namespace dev;
using namespace eth;

#define DRIVER_CUDA 1
#define DRIVER_OCL  2

#define DAG_LOAD_MODE_PARALLEL	 0
#define DAG_LOAD_MODE_SEQUENTIAL 1
#define DAG_LOAD_MODE_SINGLE	 2

#if ETH_ETHASHCL
inline void cl_configure(int devicesCount, int m_dagLoadMode, int m_dagCreateDevice) {
    unsigned m_openclPlatform = 0;
    bool m_exit = false;
    unsigned m_openclSelectedKernel = 0;  ///< A numeric value for the selected OpenCL kernel
    unsigned m_openclDeviceCount = devicesCount;
    vector<unsigned> m_openclDevices = vector<unsigned>(MAX_MINERS, -1);
    unsigned m_openclThreadsPerHash = 8;
    unsigned m_globalWorkSizeMultiplier = CLMiner::c_defaultGlobalWorkSizeMultiplier;
    unsigned m_localWorkSize = CLMiner::c_defaultLocalWorkSize;

    if (m_openclDeviceCount > 0)
    {
        CLMiner::setDevices(m_openclDevices, m_openclDeviceCount);
    }

    CLMiner::setCLKernel(m_openclSelectedKernel);
    CLMiner::setThreadsPerHash(m_openclThreadsPerHash);

    if (!CLMiner::configureGPU(
            m_localWorkSize,
            m_globalWorkSizeMultiplier,
            m_openclPlatform,
            0,
            m_dagLoadMode,
            m_dagCreateDevice,
            m_exit))
        exit(1);

    CLMiner::setNumInstances(m_openclDeviceCount);
}
#endif

extern "C" {
    void progpow_gpu_configure(uint32_t devicesCount) {
	    unsigned m_miningThreads = UINT_MAX;
        int m_dagLoadMode = DAG_LOAD_MODE_SEQUENTIAL;
        int m_dagCreateDevice = 1;

        #if ETH_ETHASHCL
            cl_configure(devicesCount, m_dagLoadMode, m_dagCreateDevice);
        #endif

        #if ETH_ETHASHCUDA
        unsigned m_cudaDeviceCount = devicesCount;
        vector<unsigned> m_cudaDevices = vector<unsigned>(MAX_MINERS, -1);
        unsigned m_numStreams = CUDAMiner::c_defaultNumStreams;
        unsigned m_cudaSchedule = 4; // sync
        unsigned m_cudaGridSize = CUDAMiner::c_defaultGridSize;
        unsigned m_cudaBlockSize = CUDAMiner::c_defaultBlockSize;
        unsigned m_parallelHash = 4;

        if (m_cudaDeviceCount > 0)
        {
            CUDAMiner::setDevices(m_cudaDevices, m_cudaDeviceCount);
            m_miningThreads = m_cudaDeviceCount;
        }

        CUDAMiner::setNumInstances(m_miningThreads);
        if (!CUDAMiner::configureGPU(
                m_cudaBlockSize,
                m_cudaGridSize,
                m_numStreams,
                m_cudaSchedule,
                0,
                m_dagLoadMode,
                m_dagCreateDevice,
                false,
                false
            ))
            exit(1);

        CUDAMiner::setParallelHash(m_parallelHash);
        #endif
    }

    void* progpow_gpu_init(unsigned device, unsigned driver) {
        void* miner = NULL;

        #if ETH_ETHASHCUDA
        if (driver == DRIVER_CUDA){
            miner = (void*)new CUDAMiner(device);
        }
        #endif

        #if ETH_ETHASHCL
        if (driver == DRIVER_OCL){
            miner = (void*)new CLMiner(device);
        }
        #endif

        if (miner == NULL) {
            std::cout << ERROR_NOT_GPU << std::endl;
        }

        return miner;
    }

    void progpow_gpu_compute(void* miner, const void* header, uint64_t height, int epoch, uint64_t boundary, uint64_t startNonce) {
        if (miner == NULL){
            exit(1);
        }

        return ((Miner*) miner)->compute(header, height, epoch, boundary, startNonce);
    }

    bool progpow_gpu_get_solutions(void* miner, void* data) {
        if (miner == NULL){
            exit(1);
        }

        return ((Miner*) miner)->get_solutions(data);
    }

    bool progpow_destroy(void* miner) {
        if (miner != NULL){
            #if ETH_ETHASHCL
                free((CLMiner*) miner);
            #endif
            #if ETH_ETHASHCUDA
                free((CUDAMiner*) miner);
            #endif
            return true;
        }
    }
}