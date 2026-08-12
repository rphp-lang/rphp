<?php

require __DIR__ . '/vendor/autoload.php';

use Symfony\Component\EventDispatcher\EventDispatcher;
use Symfony\Contracts\EventDispatcher\Event;

final class FixtureEvent extends Event
{
}

final class FixtureListeners
{
    public static $trace = '';

    public static function high(FixtureEvent $event): void
    {
        self::$trace .= 'high>';
    }

    public static function low(FixtureEvent $event): void
    {
        self::$trace .= 'low';
    }
}

$dispatcher = new EventDispatcher();
$dispatcher->addListener(FixtureEvent::class, [FixtureListeners::class, 'low'], -10);
$dispatcher->addListener(FixtureEvent::class, [FixtureListeners::class, 'high'], 10);
$event = new FixtureEvent();
$result = $dispatcher->dispatch($event);

echo FixtureListeners::$trace, '|', $result === $event ? 'same' : 'different', "\n";
