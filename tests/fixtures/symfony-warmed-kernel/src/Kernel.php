<?php

namespace Rphp\SymfonyKernelFixture;

use Symfony\Bundle\FrameworkBundle\FrameworkBundle;
use Symfony\Bundle\FrameworkBundle\Kernel\MicroKernelTrait;
use Symfony\Component\Config\Loader\LoaderInterface;
use Symfony\Component\DependencyInjection\ContainerBuilder;
use Symfony\Component\HttpKernel\Kernel as BaseKernel;
use Symfony\Component\Routing\Loader\Configurator\RoutingConfigurator;

final class Kernel extends BaseKernel
{
    use MicroKernelTrait;

    public function registerBundles(): iterable
    {
        yield new FrameworkBundle();
    }

    protected function configureContainer(
        ContainerBuilder $container,
        LoaderInterface $loader,
    ): void {
        $container->loadFromExtension('framework', [
            'secret' => 'rphp-warmed-kernel-fixture',
            'http_method_override' => false,
            'handle_all_throwables' => true,
            'router' => [
                'utf8' => true,
            ],
        ]);

        $container
            ->register(HealthController::class, HealthController::class)
            ->setPublic(true);
    }

    protected function configureRoutes(RoutingConfigurator $routes): void
    {
        $routes
            ->add('health', '/health')
            ->controller(HealthController::class)
            ->methods(['GET']);
    }
}
